use anyhow::{Result, anyhow};
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use unicode_normalization::UnicodeNormalization;
use url::Url;

use crate::utils::{
    build_chapter_url, clean_text, ensure_dir, fetch_html, file_exists, is_noise, sleep_seconds,
    slugify,
};

/// Attribute names that should NOT be used as fallback content when an
/// element has no inner text. These attributes carry presentation or
/// scripting hooks rather than visible text.
pub const NON_CONTENT_ATTRS: &[&str] = &[
    "class",
    "style",
    "id",
    "onmousedown",
    "onselectstart",
    "oncopy",
    "oncut",
];

/// Pre-compiled regex pulling the obfuscated `contentS` JS string from a
/// script block. `(?s)` enables single-line mode so `.` matches newlines.
static CONTENT_S_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?s)var\s+contentS\s*=\s*'(.*?)';\s*div\.innerHTML"#).unwrap());

/// Regex to extract the chapter number from a `chuong-NN/` URL fragment.
static CHUONG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"/chuong-(\d+)/?$").unwrap());

/// Result of parsing a chapter HTML page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterContent {
    /// Title of the parent novel (e.g. "Người Chồng Vô Dụng").
    pub novel_title: String,
    /// Title of this chapter (e.g. "Chương 12: ...").
    pub chapter_title: String,
    /// Ordered, deduplicated paragraphs of the chapter body.
    pub paragraphs: Vec<String>,
}

/// Replace XML/HTML special characters with named entities. Used both for
/// chapter HTML and for EPUB-generated XML.
pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Extract all text inside an element (cheerio's `.text()`), then [`clean_text`].
fn element_text(elem: &ElementRef<'_>) -> String {
    let combined: String = elem.text().collect();
    clean_text(&combined)
}

/// Try to derive a non-empty text representation for `elem`. Falls back to
/// the first attribute value (excluding presentation/scripting attributes)
/// when the inner text is empty, mirroring the TS extractor.
fn extract_text_from_element(elem: &ElementRef<'_>) -> Option<String> {
    let normal = element_text(elem);
    if !normal.is_empty() {
        return Some(normal);
    }
    for attr in elem.value().attrs() {
        let (name, value) = attr;
        if NON_CONTENT_ATTRS
            .iter()
            .any(|n| n.eq_ignore_ascii_case(name))
        {
            continue;
        }
        let candidate = clean_text(value);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }
    None
}

/// Pull the obfuscated injected paragraphs out of the page's inline JS, parse
/// them as HTML, and return cleaned non-noise paragraph texts. Returns an
/// empty vector if no `contentS` block is found.
fn extract_injected_content_from_script(full_html: &str) -> Vec<String> {
    let captures = match CONTENT_S_RE.captures(full_html) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let raw = captures.get(1).map(|m| m.as_str()).unwrap_or("");
    let js_html = raw.replace("\\'", "'").replace("\\\"", "\"");

    let doc = Html::parse_fragment(&js_html);
    let p_sel = Selector::parse("p").unwrap();
    let mut out = Vec::new();
    for p in doc.select(&p_sel) {
        if let Some(text) = extract_text_from_element(&p)
            && !is_noise(&text)
        {
            out.push(text);
        }
    }
    out
}

/// Pick the novel title from `.rv-full-story-title h1`, then the first
/// non-empty `<h1>`, defaulting to "Unknown Novel".
fn extract_novel_title(doc: &Html) -> String {
    let primary = Selector::parse(".rv-full-story-title h1").unwrap();
    if let Some(elem) = doc.select(&primary).next() {
        let text = element_text(&elem);
        if !text.is_empty() {
            return text;
        }
    }
    let h1_sel = Selector::parse("h1").unwrap();
    for elem in doc.select(&h1_sel) {
        let text = element_text(&elem);
        if !text.is_empty() {
            return text;
        }
    }
    "Unknown Novel".to_string()
}

/// Pick the chapter title from `.rv-chapt-title h2`, then the first non-empty
/// `<h1>` or `<h2>`, defaulting to "Untitled Chapter".
fn extract_chapter_title(doc: &Html) -> String {
    let primary = Selector::parse(".rv-chapt-title h2").unwrap();
    if let Some(elem) = doc.select(&primary).next() {
        let text = element_text(&elem);
        if !text.is_empty() {
            return text;
        }
    }
    let fallback = Selector::parse("h1, h2").unwrap();
    for elem in doc.select(&fallback) {
        let text = element_text(&elem);
        if !text.is_empty() {
            return text;
        }
    }
    "Untitled Chapter".to_string()
}

/// Parse a fetched chapter page and return its title, novel title, and
/// cleaned paragraphs. Errors when the `.chapter-c` container is missing.
pub fn extract_full_chapter_text(full_html: &str) -> Result<ChapterContent> {
    let doc = Html::parse_document(full_html);
    let chapter_sel = Selector::parse(".chapter-c").unwrap();
    let chapter = doc
        .select(&chapter_sel)
        .next()
        .ok_or_else(|| anyhow!("Could not find .chapter-c in the HTML"))?;

    let novel_title = extract_novel_title(&doc);
    let chapter_title = extract_chapter_title(&doc);
    let injected = extract_injected_content_from_script(full_html);

    let p_sel = Selector::parse("p").unwrap();
    let mut lines: Vec<String> = Vec::new();
    for child in chapter.children() {
        let elem = match ElementRef::wrap(child) {
            Some(e) => e,
            None => continue,
        };
        let tag = elem.value().name();
        let id = elem.value().attr("id");
        if tag == "div" && id == Some("data-content-truyen-backup") {
            for line in &injected {
                if !line.is_empty() && !is_noise(line) {
                    lines.push(line.clone());
                }
            }
            continue;
        }
        if tag == "p" || tag == "span" {
            if let Some(text) = extract_text_from_element(&elem)
                && !is_noise(&text)
            {
                lines.push(text);
            }
            continue;
        }
        for descendant in elem.select(&p_sel) {
            if let Some(text) = extract_text_from_element(&descendant)
                && !is_noise(&text)
            {
                lines.push(text);
            }
        }
    }

    let mut normalized: Vec<String> = Vec::new();
    for line in lines {
        let cleaned = clean_text(&line);
        if cleaned.is_empty() || is_noise(&cleaned) {
            continue;
        }
        if normalized
            .last()
            .map(|prev| prev == &cleaned)
            .unwrap_or(false)
        {
            continue;
        }
        normalized.push(cleaned);
    }

    Ok(ChapterContent {
        novel_title,
        chapter_title,
        paragraphs: normalized,
    })
}

/// Render the saved-on-disk chapter HTML document used by the EPUB importer.
/// Both titles and every paragraph are HTML-escaped before insertion.
pub fn build_html_document(
    novel_title: &str,
    chapter_title: &str,
    paragraphs: &[String],
) -> String {
    let safe_novel = escape_html(novel_title);
    let safe_chapter = escape_html(chapter_title);
    let body = paragraphs
        .iter()
        .map(|p| format!("        <p>{}</p>", escape_html(p)))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "<!DOCTYPE html>\n\
<html lang=\"vi\">\n\
<head>\n\
    <meta charset=\"UTF-8\">\n\
    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n\
    <title>{safe_chapter}</title>\n\
    <link\n        href=\"https://fonts.googleapis.com/css2?family=Literata&display=swap\"\n        rel=\"stylesheet\"\n    >\n\
    <style>\n\
        body {{\n\
            margin: 0;\n\
            padding: 0;\n\
            background: #f6f1e7;\n\
            color: #222;\n\
            font-family: \"Bookerly\", \"Literata\", \"Georgia\", \"Times New Roman\", serif;\n\
            line-height: 1.9;\n\
        }}\n\
\n\
        .container {{\n\
            max-width: 860px;\n\
            margin: 0 auto;\n\
            padding: 48px 28px 72px;\n\
        }}\n\
\n\
        .novel-title {{\n\
            text-align: center;\n\
            font-size: 1rem;\n\
            color: #666;\n\
            margin-bottom: 12px;\n\
        }}\n\
\n\
        .chapter-title {{\n\
            text-align: center;\n\
            font-size: 2.2rem;\n\
            font-weight: 700;\n\
            line-height: 1.3;\n\
            margin: 0 0 36px;\n\
        }}\n\
\n\
        .chapter-content p {{\n\
            font-size: 1.2rem;\n\
            margin: 0 0 1.15em;\n\
            text-align: justify;\n\
            text-indent: 2em;\n\
        }}\n\
    </style>\n\
</head>\n\
<body>\n\
    <div class=\"container\">\n\
        <div class=\"novel-title\">{safe_novel}</div>\n\
        <h1 class=\"chapter-title\">{safe_chapter}</h1>\n\
        <div class=\"chapter-content\">\n\
{body}\n\
        </div>\n\
    </div>\n\
</body>\n\
</html>"
    )
}

/// Walk forward through siblings starting at `start` and return the first
/// element-typed sibling matching `tag_name`. Used to mimic cheerio's
/// `.next("div")` traversal in `discover_last_chapter_number`.
fn next_sibling_element_of<'a>(
    start: ego_tree::NodeRef<'a, scraper::Node>,
    tag_name: &str,
) -> Option<ElementRef<'a>> {
    let mut sibling = start.next_sibling();
    while let Some(node) = sibling {
        if let Some(elem) = ElementRef::wrap(node)
            && elem.value().name() == tag_name
        {
            return Some(elem);
        }
        sibling = node.next_sibling();
    }
    None
}

/// Compare two strings under NFC normalization (the TS code does the same to
/// guard against pre-composed vs decomposed Vietnamese characters).
fn nfc_eq(a: &str, b: &str) -> bool {
    a.nfc().collect::<String>() == b.nfc().collect::<String>()
}

/// Pure variant of [`discover_last_chapter_number`] that takes the already
/// fetched HTML plus the absolute main-page URL. Lets callers (e.g. the TUI
/// loading screen) reuse a single `fetch_html` call for both novel-title
/// extraction and last-chapter discovery.
pub fn discover_last_chapter_number_from_html(html_source: &str, main_url: &str) -> Result<u32> {
    let doc = Html::parse_document(html_source);

    let h3_sel = Selector::parse("h3").unwrap();
    let mut latest_heading: Option<ElementRef<'_>> = None;
    for elem in doc.select(&h3_sel) {
        let text = clean_text(&elem.text().collect::<String>());
        if nfc_eq(&text, "Chương Mới Nhất") {
            latest_heading = Some(elem);
            break;
        }
    }

    let heading =
        latest_heading.ok_or_else(|| anyhow!("Could not find the 'Chương Mới Nhất' section."))?;

    let parent_node = heading
        .parent()
        .ok_or_else(|| anyhow!("Could not find the container for 'Chương Mới Nhất'."))?;

    let parent_elem = ElementRef::wrap(parent_node)
        .filter(|e| e.value().name() == "div")
        .ok_or_else(|| anyhow!("Could not find the container for 'Chương Mới Nhất'."))?;

    let chapter_list = next_sibling_element_of(*parent_elem, "div")
        .ok_or_else(|| anyhow!("Could not find the chapter list next to 'Chương Mới Nhất'."))?;

    let li_sel = Selector::parse("ul li").unwrap();
    let last_li = chapter_list
        .select(&li_sel)
        .last()
        .ok_or_else(|| anyhow!("Could not find any latest chapter entries."))?;

    let a_sel = Selector::parse("a[href]").unwrap();
    let last_link = last_li
        .select(&a_sel)
        .next()
        .ok_or_else(|| anyhow!("Could not find a link for the last chapter entry."))?;

    let href = last_link
        .value()
        .attr("href")
        .ok_or_else(|| anyhow!("Could not find a link for the last chapter entry."))?;

    let main = Url::parse(main_url)?;
    let absolute = main.join(href)?;
    let absolute_str = absolute.as_str();
    let captures = CHUONG_RE
        .captures(absolute_str)
        .ok_or_else(|| anyhow!("Could not extract the last chapter number from {absolute_str}."))?;
    let number: u32 = captures[1].parse()?;
    Ok(number)
}

/// Discover the last available chapter number by scraping the novel's main
/// page. Looks for the "Chương Mới Nhất" heading and the chapter list that
/// appears immediately after it. Performs one [`fetch_html`] call.
pub async fn discover_last_chapter_number(base_url: &str) -> Result<u32> {
    let trimmed = base_url.trim_end_matches('/');
    let main_url = format!("{}/", trimmed);
    let html_source = fetch_html(&main_url).await?;
    discover_last_chapter_number_from_html(&html_source, &main_url)
}

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

/// Build the destination chapter file path for a given novel + chapter.
fn chapter_output_path(
    output_root: &Path,
    novel_title: &str,
    chapter_number: u32,
) -> (PathBuf, PathBuf) {
    let novel_slug = slugify(novel_title, "novel");
    let output_dir = output_root.join(novel_slug);
    let file_name = format!("chapter_{:04}.html", chapter_number);
    let output_path = output_dir.join(file_name);
    (output_dir, output_path)
}

/// Effective save action after applying both per-call and run-wide policies
/// to the destination file's existence state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SaveAction {
    Write,
    Skip,
    SkipAll,
}

/// Resolve the on-disk save action for a candidate output path. Triggers the
/// `prompt` callback only when the policy is `Ask` and the file exists.
async fn resolve_existing_file_action(
    output_path: &Path,
    if_exists: ExistingFilePolicy,
    existing_policy: ExistingFilePolicy,
    prompt: &(dyn Fn(&Path) -> ExistingChapterDecision + Send + Sync),
) -> SaveAction {
    if !file_exists(output_path).await {
        return SaveAction::Write;
    }
    if existing_policy == ExistingFilePolicy::SkipAll {
        return SaveAction::Skip;
    }
    match if_exists {
        ExistingFilePolicy::Skip => SaveAction::Skip,
        ExistingFilePolicy::Overwrite => SaveAction::Write,
        ExistingFilePolicy::SkipAll => SaveAction::Skip,
        ExistingFilePolicy::Ask => match prompt(output_path) {
            ExistingChapterDecision::Redownload => SaveAction::Write,
            ExistingChapterDecision::Skip => SaveAction::Skip,
            ExistingChapterDecision::SkipAll => SaveAction::SkipAll,
        },
    }
}

/// Persist the chapter HTML on disk according to the resolved save action.
/// Returns `(output_dir, output_path, status)`.
async fn save_chapter_file(
    output_root: &Path,
    novel_title: &str,
    chapter_number: u32,
    html_doc: &str,
    if_exists: ExistingFilePolicy,
    existing_policy: ExistingFilePolicy,
    prompt: &(dyn Fn(&Path) -> ExistingChapterDecision + Send + Sync),
) -> Result<(PathBuf, PathBuf, CrawlStatus)> {
    let (output_dir, output_path) = chapter_output_path(output_root, novel_title, chapter_number);
    ensure_dir(&output_dir).await?;
    let action =
        resolve_existing_file_action(&output_path, if_exists, existing_policy, prompt).await;
    let status = match action {
        SaveAction::Write => {
            tokio::fs::write(&output_path, html_doc.as_bytes()).await?;
            CrawlStatus::Written
        }
        SaveAction::Skip => CrawlStatus::Skipped,
        SaveAction::SkipAll => CrawlStatus::SkipAll,
    };
    Ok((output_dir, output_path, status))
}

/// Download (or skip) a single chapter, parse it, save it as HTML and return
/// the resulting [`CrawlResult`]. Honours `fast_skip` to bypass the network
/// when the destination already exists.
pub async fn crawl_chapter(params: CrawlChapterParams<'_>) -> Result<CrawlResult> {
    let CrawlChapterParams {
        base_url,
        chapter_number,
        output_root,
        if_exists,
        existing_policy,
        delay,
        novel_title,
        fast_skip,
        prompt,
    } = params;

    if fast_skip && let Some(title) = novel_title {
        let (output_dir, output_path) = chapter_output_path(output_root, title, chapter_number);
        let action =
            resolve_existing_file_action(&output_path, if_exists, existing_policy, &*prompt).await;
        let status = match action {
            SaveAction::Skip => Some(CrawlStatus::Skipped),
            SaveAction::SkipAll => Some(CrawlStatus::SkipAll),
            SaveAction::Write => None,
        };
        if let Some(status) = status {
            return Ok(CrawlResult {
                novel_title: title.to_string(),
                output_dir,
                output_path,
                status,
            });
        }
    }

    let url = build_chapter_url(base_url, chapter_number);
    let full_html = fetch_html(&url).await?;
    let chapter = extract_full_chapter_text(&full_html)?;

    if chapter.paragraphs.is_empty() {
        return Err(anyhow!("No chapter content extracted from {url}"));
    }

    let html_doc = build_html_document(
        &chapter.novel_title,
        &chapter.chapter_title,
        &chapter.paragraphs,
    );

    let (output_dir, output_path, status) = save_chapter_file(
        output_root,
        &chapter.novel_title,
        chapter_number,
        &html_doc,
        if_exists,
        existing_policy,
        &*prompt,
    )
    .await?;

    if status == CrawlStatus::Written {
        sleep_seconds(delay).await;
    }

    Ok(CrawlResult {
        novel_title: chapter.novel_title,
        output_dir,
        output_path,
        status,
    })
}
