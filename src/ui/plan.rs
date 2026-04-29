use std::path::PathBuf;

use crate::crawler::ExistingFilePolicy;

/// Top-level operating mode chosen during the interactive flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrawlMode {
    /// Download chapters but do not build an EPUB.
    Crawl,
    /// Download chapters and then build an EPUB.
    CrawlEpub,
    /// Skip downloading and only build the EPUB from existing files.
    EpubOnly,
}

/// Outcome of the interactive flow when the user confirms the plan.
#[derive(Debug, Clone)]
pub struct InteractivePlan {
    /// Novel base URL.
    pub base_url: String,
    /// Operating mode chosen by the user.
    pub mode: CrawlMode,
    /// Resolved output root directory.
    pub output_root: PathBuf,
    /// Chapter range, or `None` for `EpubOnly` mode.
    pub chapter_numbers: Option<Vec<u32>>,
    /// Sleep between successful chapter writes.
    pub delay: f64,
    /// Concurrency level.
    pub workers: usize,
    /// Whether an EPUB should be built.
    pub epub: bool,
    /// Existing chapter directory override (used by `EpubOnly`).
    pub chapter_dir: Option<PathBuf>,
    /// Optional embedded font override.
    pub font_path: Option<PathBuf>,
    /// Existing-file policy.
    pub if_exists: ExistingFilePolicy,
    /// Fast-skip flag.
    pub fast_skip: bool,
    /// Discovered novel title (for fast-skip path resolution).
    pub novel_title: Option<String>,
}

/// Holds the ratatui terminal and ensures the alternate screen + raw mode
/// state are restored on drop, even on panic.
pub struct SummaryParams<'a> {
    /// Resolved novel base URL.
    pub base_url: &'a str,
    /// Operating mode chosen by the user.
    pub mode: CrawlMode,
    /// Output root directory.
    pub output_root: &'a std::path::Path,
    /// Chapter range, or `None` for `EpubOnly` mode.
    pub chapter_numbers: Option<&'a [u32]>,
    /// Sleep between successful chapter writes.
    pub delay: f64,
    /// Concurrency level.
    pub workers: usize,
    /// Existing-file policy.
    pub if_exists: ExistingFilePolicy,
    /// Existing chapter directory override.
    pub chapter_dir: Option<&'a std::path::Path>,
    /// Optional embedded font override.
    pub font_path: Option<&'a std::path::Path>,
    /// Fast-skip flag.
    pub fast_skip: bool,
}

/// Render the plan summary text shown before confirmation.
pub fn build_summary(params: SummaryParams<'_>) -> String {
    let SummaryParams {
        base_url,
        mode,
        output_root,
        chapter_numbers,
        delay,
        workers,
        if_exists,
        chapter_dir,
        font_path,
        fast_skip,
    } = params;
    let mode_label = match mode {
        CrawlMode::Crawl => "Crawl chapters",
        CrawlMode::CrawlEpub => "Crawl chapters and build EPUB",
        CrawlMode::EpubOnly => "Build EPUB from existing chapters",
    };
    let if_exists_label = match if_exists {
        ExistingFilePolicy::Ask => "ask",
        ExistingFilePolicy::Skip => "skip",
        ExistingFilePolicy::Overwrite => "overwrite",
        ExistingFilePolicy::SkipAll => "skip-all",
    };
    let mut lines = vec![
        format!("Base URL: {}", base_url),
        format!("Mode: {}", mode_label),
        format!("Output root: {}", output_root.display()),
    ];

    let has_chapter_range = matches!(chapter_numbers, Some(c) if !c.is_empty());
    if let Some(chapters) = chapter_numbers
        && let (Some(first), Some(last)) = (chapters.first(), chapters.last())
    {
        lines.push(format!(
            "Chapters: {} -> {} ({} total)",
            first,
            last,
            chapters.len()
        ));
    }

    // Always show the per-run knobs whenever a download stage is part of the
    // plan, so the user can verify their choices on one screen.
    if mode != CrawlMode::EpubOnly || has_chapter_range {
        lines.push(format!("Workers: {}", workers));
        lines.push(format!("Delay: {}s", delay));
        lines.push(format!("If chapter exists: {}", if_exists_label));
        lines.push(format!(
            "Fast skip: {}",
            if fast_skip { "yes" } else { "no" }
        ));
    }

    if let Some(dir) = chapter_dir {
        lines.push(format!("Chapter directory: {}", dir.display()));
    }

    let build_epub = mode != CrawlMode::Crawl;
    lines.push(format!(
        "Build EPUB: {}",
        if build_epub { "yes" } else { "no" }
    ));
    if build_epub {
        let font_line = match font_path {
            Some(p) => format!("Font path: {}", p.display()),
            None => "Font path: default packaged font".into(),
        };
        lines.push(font_line);
    }
    lines.join("\n")
}
