use anyhow::{Result, anyhow};
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::utils::{clean_text, is_noise};

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
