use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Policy describing what to do when the chapter file already exists on disk.
/// Mirrors the TS `ExistingFilePolicy` enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExistingFilePolicy {
    /// Ask the user interactively for each existing chapter.
    Ask,
    /// Always skip the chapter without re-downloading.
    Skip,
    /// Always overwrite the chapter with fresh content.
    Overwrite,
    /// Skip this chapter and every later existing chapter for this run.
    SkipAll,
}

/// Decision returned by the interactive existing-chapter prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExistingChapterDecision {
    /// Re-download and overwrite this chapter.
    Redownload,
    /// Skip this chapter only.
    Skip,
    /// Skip this chapter and all later existing chapters in this run.
    SkipAll,
}

/// Final state of a [`crawl_chapter`] call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrawlStatus {
    /// A new file was written to disk.
    Written,
    /// The existing file was preserved (Skip policy).
    Skipped,
    /// The existing file was preserved AND every following chapter
    /// should be skipped without prompting (SkipAll propagation).
    SkipAll,
}

/// All inputs needed to crawl a single chapter URL into a saved HTML file.
pub struct CrawlChapterParams<'a> {
    /// Novel base URL (no trailing slash required).
    pub base_url: &'a str,
    /// One-based chapter number to fetch.
    pub chapter_number: u32,
    /// Root directory under which the per-novel chapter folder is created.
    pub output_root: &'a Path,
    /// Policy applied to a single existing destination file.
    pub if_exists: ExistingFilePolicy,
    /// Run-wide existing-file policy carried across calls (used to propagate
    /// `SkipAll` once the user chooses it).
    pub existing_policy: ExistingFilePolicy,
    /// Seconds to sleep after a successful write (rate limiting).
    pub delay: f64,
    /// Pre-discovered novel title (lets `fast_skip` short-circuit before
    /// fetching the URL).
    pub novel_title: Option<&'a str>,
    /// When true and `novel_title` is provided, skip the remote fetch
    /// entirely if the destination file already exists.
    pub fast_skip: bool,
    /// Callback invoked when the policy is `Ask` and the file exists.
    pub prompt: Arc<dyn Fn(&Path) -> ExistingChapterDecision + Send + Sync>,
}

/// Outcome of [`crawl_chapter`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrawlResult {
    /// Resolved novel title (from the page title or a passed-in hint).
    pub novel_title: String,
    /// Per-novel directory beneath the configured output root.
    pub output_dir: PathBuf,
    /// Full path to the chapter file on disk.
    pub output_path: PathBuf,
    /// What happened during this call.
    pub status: CrawlStatus,
}
