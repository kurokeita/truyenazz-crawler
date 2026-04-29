use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{Html, Selector};
use std::path::Path;
use url::Url;

use crate::utils::clean_text;

/// Pre-compiled regex matching the trailing " - truyenazz" suffix on novel
/// page titles.
static TRUYENAZZ_SUFFIX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\s*-\s*truyenazz\s*$").unwrap());

/// Pre-compiled regex pulling the author name from the page body text.
static AUTHOR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)Tác giả:\s*([^\n\r]+)").unwrap());

/// Extract the novel title from the main page HTML, preferring the first
/// `<h1>` then a cleaned-up `<title>` tag, defaulting to "Unknown Novel".
pub fn extract_novel_title_from_main_page(html_source: &str) -> String {
    let doc = Html::parse_document(html_source);

    let h1 = Selector::parse("h1").unwrap();
    if let Some(elem) = doc.select(&h1).next() {
        let text = clean_text(&elem.text().collect::<String>());
        if !text.is_empty() {
            return text;
        }
    }

    let title = Selector::parse("title").unwrap();
    if let Some(elem) = doc.select(&title).next() {
        let text = clean_text(&elem.text().collect::<String>());
        if !text.is_empty() {
            return TRUYENAZZ_SUFFIX.replace(&text, "").to_string();
        }
    }
    "Unknown Novel".to_string()
}

/// Try to find the author name in the body text, returning None when the
/// "Tác giả:" prefix is absent. Trims any trailing genre text.
/// Extract the publication status (e.g. "Đang ra", "Hoàn thành") from the
/// main page. Looks for `div.content1 div.info p span.status`.
pub fn extract_novel_status_from_main_page(html_source: &str) -> Option<String> {
    let doc = Html::parse_document(html_source);
    let sel = Selector::parse("div.content1 div.info p span.status").ok()?;
    let elem = doc.select(&sel).next()?;
    let text = clean_text(&elem.text().collect::<String>());
    if text.is_empty() { None } else { Some(text) }
}

/// Extract the short novel description from the main page. The description
/// lives in a `<p>` that is the second element-level sibling of
/// `div.content1 div.info` (i.e. one element separates them).
pub fn extract_novel_description_from_main_page(html_source: &str) -> Option<String> {
    let doc = Html::parse_document(html_source);
    let info_sel = Selector::parse("div.content1 div.info").ok()?;
    let info = doc.select(&info_sel).next()?;
    let mut sibling = info.next_sibling();
    let mut element_count = 0usize;
    while let Some(node) = sibling {
        if let Some(elem) = scraper::ElementRef::wrap(node) {
            element_count += 1;
            if element_count == 2 {
                if elem.value().name() != "p" {
                    return None;
                }
                let text = clean_text(&elem.text().collect::<String>());
                return if text.is_empty() { None } else { Some(text) };
            }
        }
        sibling = node.next_sibling();
    }
    None
}

/// Extract the author name from the novel's main page, trimming any trailing
/// "Thể loại:" suffix and surrounding punctuation. Returns `None` when the
/// author is not present or resolves to an empty string.
pub fn extract_author_from_main_page(html_source: &str) -> Option<String> {
    let doc = Html::parse_document(html_source);
    let body_text: String = doc.root_element().text().collect();
    let captures = AUTHOR_RE.captures(&body_text)?;
    let raw = captures.get(1)?.as_str();
    let cleaned = clean_text(raw);
    let trimmed = cleaned
        .split("Thể loại:")
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches(|c: char| c == ',' || c.is_whitespace())
        .to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Find a cover image URL on the main page, walking the same selector
/// preference order as the TS implementation. Resolves relative URLs against
/// `novel_main_url`.
pub fn extract_cover_image_url(novel_main_url: &str, html_source: &str) -> Option<String> {
    let doc = Html::parse_document(html_source);
    let selectors = [
        "img.lazyloaded",
        "img.lazyload",
        ".book-img img",
        ".detail-info img",
        ".info-img img",
        "img",
    ];
    let base = Url::parse(novel_main_url).ok()?;
    for selector in selectors {
        let sel = Selector::parse(selector).ok()?;
        for image in doc.select(&sel) {
            let src = image
                .value()
                .attr("src")
                .or_else(|| image.value().attr("data-src"))
                .or_else(|| image.value().attr("data-original"))
                .or_else(|| image.value().attr("data-lazy-src"));
            let raw = match src {
                Some(value) => value.trim(),
                None => continue,
            };
            if raw.is_empty() || raw.starts_with("data:") {
                continue;
            }
            if let Ok(absolute) = base.join(raw) {
                return Some(absolute.to_string());
            }
        }
    }
    None
}

/// Pick the file extension to use for the embedded cover image, preferring
/// the value implied by the response's Content-Type, then the URL path's
/// extension, then `.jpg` as a final fallback.
pub fn pick_cover_extension(cover_url: &str, media_type: &str) -> String {
    if !media_type.is_empty()
        && let Some(exts) = mime_guess::get_mime_extensions_str(media_type)
        && let Some(first) = exts.first()
    {
        return format!(".{}", first);
    }
    let url_ext = Url::parse(cover_url)
        .ok()
        .and_then(|u| {
            Path::new(u.path())
                .extension()
                .and_then(|e| e.to_str().map(|s| format!(".{}", s.to_lowercase())))
        })
        .filter(|s| !s.is_empty() && s != ".");
    url_ext.unwrap_or_else(|| ".jpg".to_string())
}
