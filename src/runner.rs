use std::path::PathBuf;
use std::sync::Arc;

use crate::crawler::{
    CrawlChapterParams, CrawlStatus, ExistingChapterDecision, ExistingFilePolicy, crawl_chapter,
};

/// One observable progress event emitted by the runners. Consumers (CLI
/// progress bar, TUI progress widget, log printer) receive a stream of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressEvent {
    /// A chapter download is about to start. `total` is the number of
    /// chapters in this run.
    Started { number: u32, total: u32 },
    /// A chapter completed successfully (written, skipped, or skip-all).
    Completed { number: u32, status: CrawlStatus },
    /// A chapter failed. The runner records the error in the outcome's
    /// `failures` list as well — this event is purely for live display.
    Failed { number: u32 },
}

/// Type alias for a thread-safe progress callback.
pub type ProgressCallback = Arc<dyn Fn(ProgressEvent) + Send + Sync>;

/// Aggregated outcome of running multiple chapter downloads.
#[derive(Debug, Clone)]
pub struct RunnerOutcome {
    /// First successfully resolved per-novel directory, if any chapter ran.
    pub output_dir: Option<PathBuf>,
    /// `(chapter_number, error_message)` for each failure, sorted by chapter
    /// number for parallel runs.
    pub failures: Vec<(u32, String)>,
}

/// Inputs to [`crawl_chapters_sequential`].
pub struct SequentialParams {
    /// Chapter numbers to fetch in order.
    pub chapter_numbers: Vec<u32>,
    /// Novel base URL (no trailing slash required).
    pub base_url: String,
    /// Root directory for the per-novel output folder.
    pub output_root: PathBuf,
    /// Per-call existing-file policy. The runner additionally promotes the
    /// run-wide policy to `SkipAll` once the user (or fast-skip) chooses it.
    pub if_exists: ExistingFilePolicy,
    /// Seconds to sleep between successful chapter writes.
    pub delay: f64,
    /// Pre-discovered novel title, enabling fast-skip without a remote fetch.
    pub novel_title: Option<String>,
    /// When true, short-circuit the network call if the destination exists.
    pub fast_skip: bool,
    /// Callback invoked when the policy is `Ask` and the file exists.
    pub prompt: Arc<dyn Fn(&std::path::Path) -> ExistingChapterDecision + Send + Sync>,
    /// Optional progress observer. `None` disables progress emission.
    pub progress: Option<ProgressCallback>,
}

/// Emit a progress event if a callback is configured.
fn emit(progress: &Option<ProgressCallback>, event: ProgressEvent) {
    if let Some(cb) = progress {
        cb(event);
    }
}

/// Crawl chapters one at a time, propagating any `SkipAll` decision to
/// suppress prompts on subsequent existing chapters.
pub async fn crawl_chapters_sequential(params: SequentialParams) -> RunnerOutcome {
    let SequentialParams {
        chapter_numbers,
        base_url,
        output_root,
        if_exists,
        delay,
        novel_title,
        fast_skip,
        prompt,
        progress,
    } = params;

    let mut output_dir: Option<PathBuf> = None;
    let mut existing_policy = ExistingFilePolicy::Ask;
    let mut failures: Vec<(u32, String)> = Vec::new();
    let total = chapter_numbers.len() as u32;

    for chapter_number in chapter_numbers {
        emit(
            &progress,
            ProgressEvent::Started {
                number: chapter_number,
                total,
            },
        );
        let result = crawl_chapter(CrawlChapterParams {
            base_url: &base_url,
            chapter_number,
            output_root: &output_root,
            if_exists,
            existing_policy,
            delay,
            novel_title: novel_title.as_deref(),
            fast_skip,
            prompt: Arc::clone(&prompt),
        })
        .await;
        match result {
            Ok(crawl) => {
                if output_dir.is_none() {
                    output_dir = Some(crawl.output_dir.clone());
                }
                if crawl.status == CrawlStatus::SkipAll {
                    existing_policy = ExistingFilePolicy::SkipAll;
                }
                emit(
                    &progress,
                    ProgressEvent::Completed {
                        number: chapter_number,
                        status: crawl.status,
                    },
                );
            }
            Err(error) => {
                failures.push((chapter_number, error.to_string()));
                emit(
                    &progress,
                    ProgressEvent::Failed {
                        number: chapter_number,
                    },
                );
            }
        }
    }

    RunnerOutcome {
        output_dir,
        failures,
    }
}

/// Inputs to [`crawl_chapters_parallel`].
pub struct ParallelParams {
    /// Chapter numbers to fetch (order is not preserved across workers, but
    /// failures are sorted on return).
    pub chapter_numbers: Vec<u32>,
    /// Novel base URL.
    pub base_url: String,
    /// Output root directory.
    pub output_root: PathBuf,
    /// Per-call existing-file policy. Must NOT be `Ask` when running in
    /// parallel — the CLI guards against this.
    pub if_exists: ExistingFilePolicy,
    /// Concurrent worker count (>= 1).
    pub workers: usize,
    /// Pre-discovered novel title, enabling fast-skip.
    pub novel_title: Option<String>,
    /// When true, short-circuit on existing files.
    pub fast_skip: bool,
    /// Prompt callback (kept for API symmetry; the CLI never lets `Ask` reach
    /// the parallel path).
    pub prompt: Arc<dyn Fn(&std::path::Path) -> ExistingChapterDecision + Send + Sync>,
    /// Optional progress observer. `None` disables progress emission.
    pub progress: Option<ProgressCallback>,
}

/// Crawl chapters concurrently using a shared FIFO queue and `workers` async
/// workers. Failures are returned sorted by chapter number.
pub async fn crawl_chapters_parallel(params: ParallelParams) -> RunnerOutcome {
    let ParallelParams {
        chapter_numbers,
        base_url,
        output_root,
        if_exists,
        workers,
        novel_title,
        fast_skip,
        prompt,
        progress,
    } = params;

    let total = chapter_numbers.len() as u32;
    let queue = Arc::new(tokio::sync::Mutex::new(chapter_numbers));
    let output_dir: Arc<tokio::sync::Mutex<Option<PathBuf>>> =
        Arc::new(tokio::sync::Mutex::new(None));
    let failures: Arc<tokio::sync::Mutex<Vec<(u32, String)>>> =
        Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let base_url = Arc::new(base_url);
    let output_root = Arc::new(output_root);
    let novel_title = Arc::new(novel_title);

    let mut handles = Vec::new();
    for _ in 0..workers.max(1) {
        let queue = Arc::clone(&queue);
        let output_dir = Arc::clone(&output_dir);
        let failures = Arc::clone(&failures);
        let base_url = Arc::clone(&base_url);
        let output_root = Arc::clone(&output_root);
        let novel_title = Arc::clone(&novel_title);
        let prompt = Arc::clone(&prompt);
        let progress = progress.clone();

        handles.push(tokio::spawn(async move {
            loop {
                let chapter_number = {
                    let mut q = queue.lock().await;
                    if q.is_empty() {
                        break;
                    }
                    q.remove(0)
                };

                emit(
                    &progress,
                    ProgressEvent::Started {
                        number: chapter_number,
                        total,
                    },
                );
                let result = crawl_chapter(CrawlChapterParams {
                    base_url: base_url.as_str(),
                    chapter_number,
                    output_root: output_root.as_path(),
                    if_exists,
                    existing_policy: ExistingFilePolicy::Ask,
                    delay: 0.0,
                    novel_title: novel_title.as_deref(),
                    fast_skip,
                    prompt: Arc::clone(&prompt),
                })
                .await;
                match result {
                    Ok(crawl) => {
                        let mut od = output_dir.lock().await;
                        if od.is_none() {
                            *od = Some(crawl.output_dir.clone());
                        }
                        emit(
                            &progress,
                            ProgressEvent::Completed {
                                number: chapter_number,
                                status: crawl.status,
                            },
                        );
                    }
                    Err(error) => {
                        failures
                            .lock()
                            .await
                            .push((chapter_number, error.to_string()));
                        emit(
                            &progress,
                            ProgressEvent::Failed {
                                number: chapter_number,
                            },
                        );
                    }
                }
            }
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }

    let mut failures = failures.lock().await.clone();
    failures.sort_by_key(|(n, _)| *n);
    let output_dir = output_dir.lock().await.clone();
    RunnerOutcome {
        output_dir,
        failures,
    }
}
