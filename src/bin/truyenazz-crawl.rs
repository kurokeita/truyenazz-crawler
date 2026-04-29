use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use truyenazz_crawler::cli::{
    CliOptions, RawArgs, chapter_range, from_raw, validate_chapter_range, validate_shared_options,
};
use truyenazz_crawler::crawler::{
    CrawlStatus, ExistingChapterDecision, ExistingFilePolicy, discover_last_chapter_number,
};
use truyenazz_crawler::epub::{BuildEpubParams, build_epub, extract_novel_title_from_main_page};
use truyenazz_crawler::runner::{
    ParallelParams, ProgressCallback, ProgressEvent, SequentialParams, crawl_chapters_parallel,
    crawl_chapters_sequential,
};
use truyenazz_crawler::ui::{
    CrawlMode, DownloadProgress, InteractivePlan, make_tui_progress_callback, run_download_screen,
    run_interactive_flow,
};
use truyenazz_crawler::utils::{fetch_html, slugify};

/// Non-TUI prompt for existing chapter files. Reads a line from stdin and
/// maps r/s/a to the [`ExistingChapterDecision`] variants. Defaults to Skip
/// if stdin is closed or the input is unparseable.
fn cli_existing_chapter_prompt(chapter_path: &std::path::Path) -> ExistingChapterDecision {
    use std::io::{Write, stdin, stdout};
    eprintln!("[EXISTS] {}", chapter_path.display());
    eprint!("Choose: [r]edownload / [s]kip / skip [a]ll existing: ");
    let _ = stdout().flush();
    let mut buf = String::new();
    if stdin().read_line(&mut buf).is_err() {
        return ExistingChapterDecision::Skip;
    }
    match buf.trim().to_ascii_lowercase().as_str() {
        "r" | "redownload" => ExistingChapterDecision::Redownload,
        "a" | "all" | "skip_all" | "skip-all" => ExistingChapterDecision::SkipAll,
        _ => ExistingChapterDecision::Skip,
    }
}

/// Build an [`InteractivePlan`] from non-interactive CLI options. Discovers
/// the last available chapter when no explicit `--end` was provided.
async fn build_non_interactive_plan(
    base_url: String,
    options: &CliOptions,
) -> Result<InteractivePlan> {
    let mut chapter_numbers: Option<Vec<u32>> = None;
    let mut novel_title: Option<String> = None;

    if !options.epub_only {
        let main_url = format!("{}/", base_url.trim_end_matches('/'));
        let main_html = fetch_html(&main_url).await?;
        novel_title = Some(extract_novel_title_from_main_page(&main_html));

        let last = match discover_last_chapter_number(&base_url).await {
            Ok(n) => Some(n),
            Err(error) => {
                eprintln!("[WARN] could not discover latest chapter: {}", error);
                None
            }
        };
        let start = options.start.unwrap_or(1);
        let mut end = match (options.end, last) {
            (Some(e), _) => e,
            (None, Some(l)) => l,
            (None, None) => start,
        };
        if let Some(l) = last
            && end > l
        {
            eprintln!(
                "[INFO] Requested end chapter {} exceeds the last available chapter {}; stopping at {}.",
                end, l, l
            );
            end = l;
        }
        if let Some(message) = validate_chapter_range(start, end) {
            return Err(anyhow::anyhow!("{}", message.replace("Error: ", "")));
        }
        chapter_numbers = Some(chapter_range(start, end));
    }

    let mode = if options.epub_only {
        CrawlMode::EpubOnly
    } else if options.epub {
        CrawlMode::CrawlEpub
    } else {
        CrawlMode::Crawl
    };

    Ok(InteractivePlan {
        base_url,
        mode,
        output_root: PathBuf::from(&options.output_root),
        chapter_numbers,
        delay: options.delay,
        workers: options.workers,
        epub: options.epub || options.epub_only,
        chapter_dir: options.chapter_dir.as_ref().map(PathBuf::from),
        font_path: options.font_path.as_ref().map(PathBuf::from),
        if_exists: options.if_exists,
        fast_skip: options.fast_skip,
        novel_title,
    })
}

/// Build a styled indicatif progress bar with chapter counter, elapsed time,
/// and ETA. The bar is configured for `total` discrete chapter ticks.
fn build_progress_bar(total: u64) -> ProgressBar {
    let bar = ProgressBar::new(total);
    let template = "{prefix:.bold.cyan} {spinner:.cyan} [{elapsed_precise}] [{bar:40.cyan/blue}] \
                    {pos}/{len} ({percent}%) ETA {eta_precise} {wide_msg}";
    let style = ProgressStyle::with_template(template)
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("█▉▊▋▌▍▎▏ ");
    bar.set_style(style);
    bar.set_prefix("Chapters");
    bar.enable_steady_tick(std::time::Duration::from_millis(120));
    bar
}

/// Format a single-line status update for one chapter event, used as the
/// fallback when indicatif's bar is hidden (non-TTY) and as the message
/// printed alongside the bar when it is visible.
fn format_event(event: ProgressEvent) -> Option<String> {
    match event {
        ProgressEvent::Started { .. } => None,
        ProgressEvent::Completed { number, status } => {
            let label = match status {
                CrawlStatus::Written => "OK",
                CrawlStatus::Skipped => "SKIP",
                CrawlStatus::SkipAll => "SKIP-ALL",
            };
            Some(format!("[{label}] Chapter {}", number))
        }
        ProgressEvent::Failed { number } => Some(format!("[FAIL] Chapter {}", number)),
    }
}

/// Wire an indicatif [`ProgressBar`] up to runner [`ProgressEvent`]s. The
/// returned callback advances the bar, updates the `wide_msg` slot for the
/// in-flight chapter, and falls back to plain `eprintln!` lines when the bar
/// is hidden (non-TTY) so progress is always visible.
fn make_progress_callback(bar: ProgressBar) -> ProgressCallback {
    Arc::new(move |event| {
        if let ProgressEvent::Started { number, .. } = event {
            bar.set_message(format!("→ chapter {}", number));
        }
        let line = format_event(event);
        match event {
            ProgressEvent::Started { .. } => {}
            ProgressEvent::Completed { .. } | ProgressEvent::Failed { .. } => {
                if let Some(text) = line {
                    if bar.is_hidden() {
                        eprintln!("{}", text);
                    } else {
                        bar.println(text);
                    }
                }
                bar.inc(1);
            }
        }
    })
}

/// Resolve the per-novel chapter directory used for an `epub_only` run when
/// no explicit `--chapter-dir` was provided.
async fn infer_chapter_dir(base_url: &str, output_root: &std::path::Path) -> Result<PathBuf> {
    let main_url = format!("{}/", base_url.trim_end_matches('/'));
    let main_html = fetch_html(&main_url).await?;
    let title = extract_novel_title_from_main_page(&main_html);
    Ok(output_root.join(slugify(&title, "book")))
}

/// Drive a chapter run with an indicatif progress bar (non-interactive mode).
async fn run_with_indicatif(
    plan: &InteractivePlan,
    chapters: Vec<u32>,
    prompt: Arc<dyn Fn(&std::path::Path) -> ExistingChapterDecision + Send + Sync>,
) -> Result<truyenazz_crawler::runner::RunnerOutcome, i32> {
    let bar = build_progress_bar(chapters.len() as u64);
    let progress = make_progress_callback(bar.clone());
    let outcome = if plan.workers <= 1 {
        crawl_chapters_sequential(SequentialParams {
            chapter_numbers: chapters,
            base_url: plan.base_url.clone(),
            output_root: plan.output_root.clone(),
            if_exists: plan.if_exists,
            delay: plan.delay,
            novel_title: plan.novel_title.clone(),
            fast_skip: plan.fast_skip,
            prompt,
            progress: Some(progress),
        })
        .await
    } else {
        if plan.if_exists == ExistingFilePolicy::Ask {
            bar.finish_and_clear();
            eprintln!("Error: --workers > 1 requires --if-exists skip or --if-exists overwrite.");
            return Err(1);
        }
        crawl_chapters_parallel(ParallelParams {
            chapter_numbers: chapters,
            base_url: plan.base_url.clone(),
            output_root: plan.output_root.clone(),
            if_exists: plan.if_exists,
            workers: plan.workers,
            novel_title: plan.novel_title.clone(),
            fast_skip: plan.fast_skip,
            prompt,
            progress: Some(progress),
        })
        .await
    };
    bar.finish_with_message("done");
    Ok(outcome)
}

/// Drive a chapter run with the styled ratatui TUI download screen.
///
/// `wait_for_user` controls whether the screen pauses for an Enter press
/// after completion. Pass `false` when an EPUB build screen will follow so
/// the bare terminal never flashes between TUI screens.
async fn run_with_tui(
    plan: &InteractivePlan,
    chapters: Vec<u32>,
    prompt: Arc<dyn Fn(&std::path::Path) -> ExistingChapterDecision + Send + Sync>,
    wait_for_user: bool,
) -> Result<truyenazz_crawler::runner::RunnerOutcome, i32> {
    if plan.workers > 1 && plan.if_exists == ExistingFilePolicy::Ask {
        eprintln!("Error: --workers > 1 requires --if-exists skip or --if-exists overwrite.");
        return Err(1);
    }
    let total = chapters.len() as u32;
    let state = Arc::new(std::sync::Mutex::new(DownloadProgress::new(total)));
    let progress = make_tui_progress_callback(Arc::clone(&state));

    let plan_clone = plan.clone();
    let prompt_clone = Arc::clone(&prompt);
    let task = tokio::spawn(async move {
        if plan_clone.workers <= 1 {
            crawl_chapters_sequential(SequentialParams {
                chapter_numbers: chapters,
                base_url: plan_clone.base_url.clone(),
                output_root: plan_clone.output_root.clone(),
                if_exists: plan_clone.if_exists,
                delay: plan_clone.delay,
                novel_title: plan_clone.novel_title.clone(),
                fast_skip: plan_clone.fast_skip,
                prompt: prompt_clone,
                progress: Some(progress),
            })
            .await
        } else {
            crawl_chapters_parallel(ParallelParams {
                chapter_numbers: chapters,
                base_url: plan_clone.base_url.clone(),
                output_root: plan_clone.output_root.clone(),
                if_exists: plan_clone.if_exists,
                workers: plan_clone.workers,
                novel_title: plan_clone.novel_title.clone(),
                fast_skip: plan_clone.fast_skip,
                prompt: prompt_clone,
                progress: Some(progress),
            })
            .await
        }
    });

    match run_download_screen(state, task, wait_for_user).await {
        Ok(outcome) => Ok(outcome),
        Err(error) => {
            eprintln!("[FAIL] download screen error: {}", error);
            Err(1)
        }
    }
}

/// Execute a fully-resolved interactive plan: download chapters and/or build
/// the EPUB. Returns the process exit code (0 = success, 2 = partial
/// failures, 3 = EPUB build failed).
///
/// `interactive` selects the progress UI: when true, the TUI download screen
/// is shown; when false, an indicatif bar prints to stderr.
async fn execute_plan(plan: InteractivePlan, interactive: bool) -> i32 {
    let prompt: Arc<dyn Fn(&std::path::Path) -> ExistingChapterDecision + Send + Sync> =
        Arc::new(cli_existing_chapter_prompt);

    let output_dir: Option<PathBuf>;
    let mut failures: Vec<(u32, String)> = Vec::new();

    if plan.mode == CrawlMode::EpubOnly {
        output_dir = match plan.chapter_dir.clone() {
            Some(dir) => Some(dir),
            None => match infer_chapter_dir(&plan.base_url, &plan.output_root).await {
                Ok(p) => Some(p),
                Err(error) => {
                    eprintln!("[FAIL] Could not infer chapter directory: {}", error);
                    return 3;
                }
            },
        };
    } else {
        let chapters = plan.chapter_numbers.clone().unwrap_or_default();
        if !interactive {
            if let (Some(first), Some(last)) = (chapters.first(), chapters.last()) {
                println!(
                    "[INFO] Downloading chapters {} -> {} ({} chapters)",
                    first,
                    last,
                    chapters.len()
                );
            }
            println!("[INFO] Using {} worker(s)", plan.workers);
        }

        let outcome = if interactive {
            // Skip the post-download "press Enter" wait when an EPUB build
            // screen is queued, so the user transitions straight from the
            // download screen into the build screen.
            match run_with_tui(&plan, chapters, Arc::clone(&prompt), !plan.epub).await {
                Ok(o) => o,
                Err(code) => return code,
            }
        } else {
            match run_with_indicatif(&plan, chapters, Arc::clone(&prompt)).await {
                Ok(o) => o,
                Err(code) => return code,
            }
        };
        output_dir = outcome.output_dir;
        failures = outcome.failures;
    }

    if !failures.is_empty() && !interactive {
        eprintln!("\nSome chapters failed:");
        for (chapter, message) in &failures {
            eprintln!("  - Chapter {}: {}", chapter, message);
        }
    }

    if plan.epub {
        let chapter_dir = match output_dir.clone() {
            Some(d) => d,
            None => {
                eprintln!("[FAIL] No chapter directory available to build EPUB.");
                return 3;
            }
        };
        let novel_main_url = format!("{}/", plan.base_url.trim_end_matches('/'));
        let font_path = plan.font_path.clone();
        let build_future = async move {
            build_epub(BuildEpubParams {
                novel_main_url,
                chapter_dir,
                output_epub: None,
                font_path,
            })
            .await
        };
        let epub_result = if interactive {
            // Stay inside the TUI: a styled "Building EPUB" screen with the
            // shared spinner runs while the build future resolves. Esc/Ctrl+C
            // aborts the build.
            match truyenazz_crawler::ui::run_loading_screen(
                "Building EPUB",
                "Packaging chapters, font, and cover into an EPUB archive…",
                build_future,
            )
            .await
            {
                Ok(truyenazz_crawler::ui::PromptOutcome::Submitted(inner)) => inner,
                Ok(truyenazz_crawler::ui::PromptOutcome::Back)
                | Ok(truyenazz_crawler::ui::PromptOutcome::Quit) => {
                    eprintln!("[INFO] EPUB build cancelled by user.");
                    return 1;
                }
                Err(error) => {
                    eprintln!("[FAIL] EPUB screen error: {}", error);
                    return 3;
                }
            }
        } else {
            build_future.await
        };
        match epub_result {
            Ok(path) => {
                if interactive {
                    let mut body = format!("EPUB created at:\n{}", path.display());
                    if !failures.is_empty() {
                        body.push_str(&format!(
                            "\n\n{} chapter(s) failed during download.",
                            failures.len()
                        ));
                    }
                    let _ = truyenazz_crawler::ui::show_note("Done", &body);
                } else {
                    println!("[OK] EPUB -> {}", path.display());
                }
            }
            Err(error) => {
                if interactive {
                    let _ = truyenazz_crawler::ui::show_note(
                        "EPUB build failed",
                        &format!("{}", error),
                    );
                } else {
                    eprintln!("[FAIL] EPUB build failed: {}", error);
                }
                return 3;
            }
        }
    }

    if failures.is_empty() { 0 } else { 2 }
}

/// Async entry point: parse CLI, dispatch interactive vs. non-interactive,
/// and run the resulting plan to completion.
async fn run() -> i32 {
    let parsed = from_raw(RawArgs::parse());

    if let Some(message) = validate_shared_options(&parsed.options) {
        eprintln!("{}", message);
        return 1;
    }

    let interactive = parsed.options.interactive || parsed.base_url.is_none();
    let plan = match parsed.base_url {
        Some(base_url) if !parsed.options.interactive => {
            match build_non_interactive_plan(base_url, &parsed.options).await {
                Ok(plan) => plan,
                Err(error) => {
                    eprintln!("Error: {}", error);
                    return 1;
                }
            }
        }
        base_url => match run_interactive_flow(base_url, &parsed.options).await {
            Ok(Some(plan)) => plan,
            Ok(None) => {
                eprintln!("Interactive crawl cancelled.");
                return 1;
            }
            Err(error) => {
                eprintln!("Error launching TUI: {}", error);
                return 1;
            }
        },
    };

    execute_plan(plan, interactive).await
}

/// Process entry point. Spins up a Tokio runtime and exits with the code
/// returned by [`run`].
#[tokio::main]
async fn main() {
    let code = run().await;
    std::process::exit(code);
}
