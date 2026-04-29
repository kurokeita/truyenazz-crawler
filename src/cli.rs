use clap::{ArgAction, Parser, ValueEnum};

use crate::crawler::ExistingFilePolicy;

/// CLI mirror of [`ExistingFilePolicy`] used for clap parsing. We keep this
/// as a separate type so the user-facing string values ("ask"/"skip"/...)
/// can be customised via clap attributes without leaking into the domain
/// type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lowercase")]
pub enum CliExistingPolicy {
    /// Prompt the user for each existing chapter file.
    Ask,
    /// Always skip existing chapter files.
    Skip,
    /// Always overwrite existing chapter files.
    Overwrite,
}

impl From<CliExistingPolicy> for ExistingFilePolicy {
    /// Lift a CLI-surface policy into the runtime `ExistingFilePolicy` used
    /// by the crawler (the CLI surface deliberately omits `SkipAll`).
    fn from(value: CliExistingPolicy) -> Self {
        match value {
            CliExistingPolicy::Ask => ExistingFilePolicy::Ask,
            CliExistingPolicy::Skip => ExistingFilePolicy::Skip,
            CliExistingPolicy::Overwrite => ExistingFilePolicy::Overwrite,
        }
    }
}

/// Raw clap-parsed command-line arguments.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "truyenazz-crawl",
    about = "Crawl a chapter range and optionally build an EPUB.",
    version
)]
pub struct RawArgs {
    /// Novel base URL, e.g. `https://truyenazz.me/your-novel`.
    pub base_url: Option<String>,

    /// Start chapter number (inclusive).
    #[arg(long)]
    pub start: Option<u32>,

    /// End chapter number (inclusive).
    #[arg(long)]
    pub end: Option<u32>,

    /// Root output directory under which `<novel>/chapter_NNNN.html` is written.
    #[arg(long, default_value = "output")]
    pub output_root: String,

    /// Delay (seconds) between sequential requests.
    #[arg(long, default_value_t = 0.5)]
    pub delay: f64,

    /// Number of concurrent download workers.
    #[arg(long, default_value_t = 1)]
    pub workers: usize,

    /// Also build an EPUB after crawling finishes.
    #[arg(long, action = ArgAction::SetTrue)]
    pub epub: bool,

    /// Skip chapter downloads and build the EPUB from existing saved HTML files.
    #[arg(long, action = ArgAction::SetTrue)]
    pub epub_only: bool,

    /// Existing chapter directory to use when building EPUB without crawling.
    #[arg(long)]
    pub chapter_dir: Option<String>,

    /// Font file to embed in the EPUB instead of the bundled font.
    #[arg(long)]
    pub font_path: Option<String>,

    /// Behaviour when a chapter file already exists locally.
    #[arg(long, value_enum, default_value_t = CliExistingPolicy::Ask)]
    pub if_exists: CliExistingPolicy,

    /// Skip checking the remote URL when the chapter file already exists.
    #[arg(long, action = ArgAction::SetTrue)]
    pub fast_skip: bool,

    /// Launch the interactive TUI.
    #[arg(short = 'i', long, action = ArgAction::SetTrue)]
    pub interactive: bool,
}

/// Normalised CLI options used by the rest of the program. Decouples the
/// internal types from the clap-derived [`RawArgs`].
#[derive(Debug, Clone)]
pub struct CliOptions {
    /// `--start` chapter, if any.
    pub start: Option<u32>,
    /// `--end` chapter, if any.
    pub end: Option<u32>,
    /// `--output-root` directory.
    pub output_root: String,
    /// `--delay` seconds.
    pub delay: f64,
    /// `--workers` concurrency level.
    pub workers: usize,
    /// `--epub` flag.
    pub epub: bool,
    /// `--epub-only` flag.
    pub epub_only: bool,
    /// `--chapter-dir` override.
    pub chapter_dir: Option<String>,
    /// `--font-path` override.
    pub font_path: Option<String>,
    /// Resolved existing-file policy.
    pub if_exists: ExistingFilePolicy,
    /// `--fast-skip` flag.
    pub fast_skip: bool,
    /// `-i` / `--interactive` flag.
    pub interactive: bool,
}

impl Default for CliOptions {
    /// Return the same defaults as the clap parser would for an args list
    /// containing only `truyenazz-crawl`.
    fn default() -> Self {
        Self {
            start: None,
            end: None,
            output_root: "output".to_string(),
            delay: 0.5,
            workers: 1,
            epub: false,
            epub_only: false,
            chapter_dir: None,
            font_path: None,
            if_exists: ExistingFilePolicy::Ask,
            fast_skip: false,
            interactive: false,
        }
    }
}

/// Parsed CLI invocation, separating the optional positional URL from the
/// flag set so callers can decide between interactive and non-interactive
/// modes uniformly.
#[derive(Debug, Clone)]
pub struct ParsedCli {
    /// Positional URL argument (`None` triggers interactive mode).
    pub base_url: Option<String>,
    /// Flag set already normalised into [`CliOptions`].
    pub options: CliOptions,
}

/// Convert a parsed [`RawArgs`] into the public [`ParsedCli`] form. Public
/// so the binary can call this after letting clap auto-exit on help/version.
pub fn from_raw(raw: RawArgs) -> ParsedCli {
    let options = CliOptions {
        start: raw.start,
        end: raw.end,
        output_root: raw.output_root,
        delay: raw.delay,
        workers: raw.workers,
        epub: raw.epub || raw.epub_only,
        epub_only: raw.epub_only,
        chapter_dir: raw.chapter_dir,
        font_path: raw.font_path,
        if_exists: raw.if_exists.into(),
        fast_skip: raw.fast_skip,
        interactive: raw.interactive,
    };
    ParsedCli {
        base_url: raw.base_url,
        options,
    }
}

/// Parse `argv` into a [`ParsedCli`]. Returns the rendered clap error message
/// as a `String` on failure so tests can inspect it without taking a clap
/// dependency. The binary uses [`from_raw`] with `RawArgs::parse()` so help
/// and version output go to stdout via clap's auto-exit path.
pub fn parse_from<I, T>(argv: I) -> Result<ParsedCli, String>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    RawArgs::try_parse_from(argv)
        .map(from_raw)
        .map_err(|e| e.to_string())
}

/// Validate cross-cutting CLI options. Returns a user-facing error message
/// when a combination is illegal.
pub fn validate_shared_options(options: &CliOptions) -> Option<String> {
    if options.workers == 0 {
        return Some("Error: --workers must be a positive integer.".to_string());
    }
    if options.workers > 1 && options.if_exists == ExistingFilePolicy::Ask {
        return Some(
            "Error: --workers > 1 requires --if-exists skip or --if-exists overwrite.".to_string(),
        );
    }
    None
}

/// Validate the start/end chapter range. Returns a user-facing error message
/// when invalid, otherwise `None`.
pub fn validate_chapter_range(start: u32, end: u32) -> Option<String> {
    if start == 0 || end == 0 {
        return Some("Error: chapter numbers must be positive integers.".to_string());
    }
    if start > end {
        return Some("Error: --start must be less than or equal to --end.".to_string());
    }
    None
}

/// Inclusive integer range as a Vec, mirroring the TS `range(start, end)` helper.
pub fn chapter_range(start: u32, end: u32) -> Vec<u32> {
    (start..=end).collect()
}
