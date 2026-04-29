use anyhow::Result;
use std::path::PathBuf;

use crate::crawler::ExistingFilePolicy;
use crate::ui::PromptOutcome;
use crate::ui::plan::{CrawlMode, InteractivePlan, SummaryParams, build_summary};
use crate::ui::screens::{
    run_confirm, run_loading_screen, run_path_prompt, run_select, run_text_prompt, show_note,
};
use crate::ui::widgets::{Select, SelectOption, Validator};

use super::state::{FontChoice, StepResult, WizardState, WizardStep};

macro_rules! advance_or_back {
    ($outcome:expr, $previous:expr, |$value:ident| $on_submit:block) => {
        match $outcome {
            PromptOutcome::Submitted($value) => $on_submit,
            PromptOutcome::Back => Ok(StepResult::Next($previous)),
            PromptOutcome::Quit => Ok(StepResult::Quit),
        }
    };
}

/// Welcome screen. Esc cancels the wizard since there is no earlier step.
pub(super) fn step_welcome(_state: &mut WizardState) -> Result<StepResult> {
    match show_note(
        "truyenazz-crawl",
        "Welcome — let's set up the crawl.\n\nPress Enter to continue, Esc/Ctrl+C to quit.",
    )? {
        PromptOutcome::Submitted(()) => Ok(StepResult::Next(WizardStep::BaseUrl)),
        PromptOutcome::Back | PromptOutcome::Quit => Ok(StepResult::Quit),
    }
}

/// Novel base URL prompt. Skipped entirely if the URL was supplied on the CLI.
pub(super) fn step_base_url(state: &mut WizardState) -> Result<StepResult> {
    if state.has_initial_url {
        return Ok(StepResult::Next(WizardStep::Mode));
    }
    let validator: Validator = Box::new(|value: &str| {
        let trimmed = value.trim();
        let valid = !trimmed.is_empty()
            && (trimmed.starts_with("http://") || trimmed.starts_with("https://"));
        if valid {
            None
        } else {
            Some("Enter a valid http:// or https:// URL.".to_string())
        }
    });
    let outcome = run_text_prompt(
        "Novel base URL",
        "Paste the novel base URL.",
        Some(state.base_url.clone()).filter(|s| !s.is_empty()),
        Some("https://truyenazz.me/your-novel"),
        Some(validator),
    )?;
    advance_or_back!(outcome, WizardStep::Welcome, |value| {
        state.base_url = value.trim().to_string();
        // Invalidate any previously cached discovery whenever the URL changes.
        state.novel_title = None;
        state.last_discovered = None;
        Ok(StepResult::Next(WizardStep::Mode))
    })
}

/// Operating-mode select.
pub(super) fn step_mode(state: &mut WizardState) -> Result<StepResult> {
    let mode_options = vec![
        SelectOption {
            label: "Crawl chapters".into(),
            value: CrawlMode::Crawl,
            hint: None,
        },
        SelectOption {
            label: "Crawl chapters and build an EPUB".into(),
            value: CrawlMode::CrawlEpub,
            hint: None,
        },
        SelectOption {
            label: "Build an EPUB from existing chapter files".into(),
            value: CrawlMode::EpubOnly,
            hint: None,
        },
    ];
    let outcome = run_select(
        "Mode",
        "What do you want to do?",
        Select::with_initial(mode_options, &state.mode),
    )?;
    advance_or_back!(outcome, WizardStep::BaseUrl, |chosen| {
        state.mode = chosen;
        Ok(StepResult::Next(WizardStep::OutputRoot))
    })
}

/// Output root prompt.
pub(super) fn step_output_root(state: &mut WizardState) -> Result<StepResult> {
    let outcome = run_text_prompt(
        "Output root",
        "Where should chapter files (and the EPUB) be saved?",
        Some(state.output_root.to_string_lossy().into_owned()),
        None,
        Some(Box::new(|v| {
            if v.trim().is_empty() {
                Some("Enter an output directory.".into())
            } else {
                None
            }
        })),
    )?;
    advance_or_back!(outcome, WizardStep::Mode, |value| {
        state.output_root = PathBuf::from(value.trim());
        let next = if state.mode == CrawlMode::EpubOnly {
            WizardStep::ChapterDir
        } else {
            WizardStep::Discover
        };
        Ok(StepResult::Next(next))
    })
}

/// Aggregated novel metadata pulled out of the main page during discovery.
struct DiscoveredNovel {
    title: Option<String>,
    last_chapter: Option<u32>,
    status: Option<String>,
    description: Option<String>,
}

/// Run the title + status + description + last-chapter discovery under a
/// styled loading screen, then show a brief novel-info note.
pub(super) async fn step_discover(state: &mut WizardState) -> Result<StepResult> {
    let url = state.base_url.clone();
    let outcome = run_loading_screen(
        "Discovering novel",
        "Fetching main page and detecting latest chapter…",
        async move {
            let main_url = format!("{}/", url.trim_end_matches('/'));
            let html = match crate::utils::fetch_html(&main_url).await {
                Ok(h) => h,
                Err(_) => {
                    return DiscoveredNovel {
                        title: None,
                        last_chapter: None,
                        status: None,
                        description: None,
                    };
                }
            };
            DiscoveredNovel {
                title: Some(crate::epub::extract_novel_title_from_main_page(&html)),
                last_chapter: crate::crawler::discover_last_chapter_number_from_html(
                    &html, &main_url,
                )
                .ok(),
                status: crate::epub::extract_novel_status_from_main_page(&html),
                description: crate::epub::extract_novel_description_from_main_page(&html),
            }
        },
    )
    .await?;
    let novel = match outcome {
        PromptOutcome::Submitted(novel) => novel,
        PromptOutcome::Back => return Ok(StepResult::Next(WizardStep::OutputRoot)),
        PromptOutcome::Quit => return Ok(StepResult::Quit),
    };
    state.novel_title = novel.title;
    state.last_discovered = novel.last_chapter;
    state.novel_status = novel.status;
    state.novel_description = novel.description;

    if state.novel_title.is_some() || state.last_discovered.is_some() {
        let mut lines: Vec<String> = Vec::new();
        if let Some(title) = state.novel_title.as_ref() {
            lines.push(format!("Title: {}", title));
        }
        if let Some(status) = state.novel_status.as_ref() {
            lines.push(format!("Status: {}", status));
        }
        if let Some(last) = state.last_discovered {
            lines.push(format!("Latest chapter: {}", last));
        }
        if let Some(desc) = state.novel_description.as_ref() {
            lines.push(String::new());
            lines.push("Description:".to_string());
            lines.push(desc.clone());
        }
        match show_note("Novel", &lines.join("\n"))? {
            PromptOutcome::Submitted(()) => {}
            PromptOutcome::Back => return Ok(StepResult::Next(WizardStep::OutputRoot)),
            PromptOutcome::Quit => return Ok(StepResult::Quit),
        }
    }
    Ok(StepResult::Next(WizardStep::StartChapter))
}

/// Start chapter prompt.
pub(super) fn step_start_chapter(state: &mut WizardState) -> Result<StepResult> {
    let validator: Validator = Box::new(|v: &str| match v.trim().parse::<u32>() {
        Ok(n) if n > 0 => None,
        _ => Some("Enter a positive integer.".into()),
    });
    let initial = if state.start_chapter == 0 {
        1
    } else {
        state.start_chapter
    };
    let outcome = run_text_prompt(
        "Start chapter",
        "First chapter to download.",
        Some(initial.to_string()),
        None,
        Some(validator),
    )?;
    advance_or_back!(outcome, WizardStep::OutputRoot, |value| {
        state.start_chapter = value.trim().parse().unwrap_or(1);
        Ok(StepResult::Next(WizardStep::EndChapter))
    })
}

/// End chapter prompt.
pub(super) fn step_end_chapter(state: &mut WizardState) -> Result<StepResult> {
    let initial = if state.end_chapter > 0 {
        state.end_chapter
    } else {
        state
            .last_discovered
            .unwrap_or_else(|| state.start_chapter.max(1))
    };
    let validator: Validator = Box::new(|v: &str| match v.trim().parse::<u32>() {
        Ok(n) if n > 0 => None,
        _ => Some("Enter a positive integer.".into()),
    });
    let outcome = run_text_prompt(
        "End chapter",
        "Last chapter to download (inclusive).",
        Some(initial.to_string()),
        None,
        Some(validator),
    )?;
    advance_or_back!(outcome, WizardStep::StartChapter, |value| {
        let parsed: u32 = value.trim().parse().unwrap_or(state.start_chapter);
        state.end_chapter = parsed;
        if let Some(message) = crate::cli::validate_chapter_range(state.start_chapter, parsed) {
            // Show the validation error and then fall back to the prior step
            // so the user can pick a valid range.
            let _ = show_note("Invalid range", &message)?;
            return Ok(StepResult::Next(WizardStep::StartChapter));
        }
        Ok(StepResult::Next(WizardStep::Workers))
    })
}

/// Workers prompt.
pub(super) fn step_workers(state: &mut WizardState) -> Result<StepResult> {
    let validator: Validator = Box::new(|v: &str| match v.trim().parse::<usize>() {
        Ok(n) if n > 0 => None,
        _ => Some("Enter a positive integer.".into()),
    });
    let outcome = run_text_prompt(
        "Workers",
        "How many download workers should run in parallel?",
        Some(state.workers.to_string()),
        None,
        Some(validator),
    )?;
    advance_or_back!(outcome, WizardStep::EndChapter, |value| {
        state.workers = value.trim().parse().unwrap_or(1).max(1);
        Ok(StepResult::Next(WizardStep::Delay))
    })
}

/// Delay prompt.
pub(super) fn step_delay(state: &mut WizardState) -> Result<StepResult> {
    let validator: Validator = Box::new(|v: &str| match v.trim().parse::<f64>() {
        Ok(n) if n >= 0.0 => None,
        _ => Some("Enter a non-negative number.".into()),
    });
    let outcome = run_text_prompt(
        "Delay",
        "Delay between requests (seconds).",
        Some(state.delay.to_string()),
        None,
        Some(validator),
    )?;
    advance_or_back!(outcome, WizardStep::Workers, |value| {
        let parsed: f64 = value.trim().parse().unwrap_or(0.0);
        state.delay = parsed.max(0.0);
        Ok(StepResult::Next(WizardStep::IfExists))
    })
}

/// Existing-file policy select. Hides the `Ask` option when running in parallel.
pub(super) fn step_if_exists(state: &mut WizardState) -> Result<StepResult> {
    let mut allowed = Vec::new();
    if state.workers <= 1 {
        allowed.push(SelectOption {
            label: "Ask what to do for each existing chapter".into(),
            value: ExistingFilePolicy::Ask,
            hint: None,
        });
    }
    allowed.push(SelectOption {
        label: "Skip existing chapter files".into(),
        value: ExistingFilePolicy::Skip,
        hint: None,
    });
    allowed.push(SelectOption {
        label: "Overwrite existing chapter files".into(),
        value: ExistingFilePolicy::Overwrite,
        hint: None,
    });
    let initial_policy = if state.workers > 1 && state.if_exists == ExistingFilePolicy::Ask {
        ExistingFilePolicy::Skip
    } else {
        state.if_exists
    };
    let outcome = run_select(
        "If chapter exists",
        "Pick a behaviour for existing chapter files.",
        Select::with_initial(allowed, &initial_policy),
    )?;
    advance_or_back!(outcome, WizardStep::Delay, |value| {
        state.if_exists = value;
        Ok(StepResult::Next(WizardStep::FastSkip))
    })
}

/// Chapter directory prompt — only used in `EpubOnly` mode.
pub(super) fn step_chapter_dir(state: &mut WizardState) -> Result<StepResult> {
    let outcome = run_text_prompt(
        "Chapter directory",
        "Path to the existing chapter directory.",
        state
            .chapter_dir
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        None,
        None,
    )?;
    advance_or_back!(outcome, WizardStep::OutputRoot, |value| {
        state.chapter_dir = Some(PathBuf::from(value.trim()));
        Ok(StepResult::Next(WizardStep::FontChoice))
    })
}

/// Fast-skip yes/no.
pub(super) fn step_fast_skip(state: &mut WizardState) -> Result<StepResult> {
    let outcome = run_confirm(
        "Fast skip",
        "Bypass the remote check when the chapter file already exists locally?",
        state.fast_skip,
    )?;
    advance_or_back!(outcome, WizardStep::IfExists, |value| {
        state.fast_skip = value;
        let next = if state.mode == CrawlMode::Crawl {
            WizardStep::Confirm
        } else {
            WizardStep::FontChoice
        };
        Ok(StepResult::Next(next))
    })
}

/// Choose between bundled vs custom EPUB font.
pub(super) fn step_font_choice(state: &mut WizardState) -> Result<StepResult> {
    let outcome = run_select(
        "EPUB font",
        "Pick the font embedded in the EPUB.",
        Select::with_initial(
            vec![
                SelectOption {
                    label: "Use the bundled Bokerlam.ttf".into(),
                    value: FontChoice::Default,
                    hint: None,
                },
                SelectOption {
                    label: "Pick a custom font file path".into(),
                    value: FontChoice::Custom,
                    hint: None,
                },
            ],
            &state.font_choice,
        ),
    )?;
    let previous = if state.mode == CrawlMode::EpubOnly {
        WizardStep::ChapterDir
    } else {
        WizardStep::FastSkip
    };
    advance_or_back!(outcome, previous, |choice| {
        state.font_choice = choice;
        let next = match choice {
            FontChoice::Custom => WizardStep::FontPath,
            FontChoice::Default => {
                state.font_path = None;
                WizardStep::Confirm
            }
        };
        Ok(StepResult::Next(next))
    })
}

/// Custom font path picker.
pub(super) fn step_font_path(state: &mut WizardState) -> Result<StepResult> {
    let outcome = run_path_prompt(
        "Font path",
        "Absolute path to the .ttf/.otf file. Tab to autocomplete.",
        state
            .font_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
    )?;
    advance_or_back!(outcome, WizardStep::FontChoice, |value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Ok(StepResult::Next(WizardStep::FontChoice));
        }
        state.font_path = Some(PathBuf::from(trimmed));
        Ok(StepResult::Next(WizardStep::Confirm))
    })
}

/// Final confirmation. Yes finalizes the plan; No goes back to the prior step.
pub(super) fn step_confirm(state: &mut WizardState) -> Result<StepResult> {
    let chapter_numbers = if state.mode == CrawlMode::EpubOnly {
        None
    } else {
        Some(crate::cli::chapter_range(
            state.start_chapter,
            state.end_chapter,
        ))
    };
    let summary = build_summary(SummaryParams {
        base_url: &state.base_url,
        mode: state.mode,
        output_root: &state.output_root,
        chapter_numbers: chapter_numbers.as_deref(),
        delay: state.delay,
        workers: state.workers,
        if_exists: state.if_exists,
        chapter_dir: state.chapter_dir.as_deref(),
        font_path: state.font_path.as_deref(),
        fast_skip: state.fast_skip,
    });
    let previous = match state.mode {
        CrawlMode::Crawl => WizardStep::FastSkip,
        CrawlMode::CrawlEpub | CrawlMode::EpubOnly => match state.font_choice {
            FontChoice::Custom => WizardStep::FontPath,
            FontChoice::Default => WizardStep::FontChoice,
        },
    };
    let outcome = run_confirm("Plan", &summary, true)?;
    match outcome {
        PromptOutcome::Submitted(true) => Ok(StepResult::Done(InteractivePlan {
            base_url: state.base_url.clone(),
            mode: state.mode,
            output_root: state.output_root.clone(),
            chapter_numbers,
            delay: state.delay,
            workers: state.workers,
            epub: state.mode != CrawlMode::Crawl,
            chapter_dir: state.chapter_dir.clone(),
            font_path: state.font_path.clone(),
            if_exists: state.if_exists,
            fast_skip: state.fast_skip,
            novel_title: state.novel_title.clone(),
        })),
        PromptOutcome::Submitted(false) => Ok(StepResult::Next(previous)),
        PromptOutcome::Back => Ok(StepResult::Next(previous)),
        PromptOutcome::Quit => Ok(StepResult::Quit),
    }
}
