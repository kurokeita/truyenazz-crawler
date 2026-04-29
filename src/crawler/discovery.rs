use anyhow::{Result, anyhow};
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use unicode_normalization::UnicodeNormalization;
use url::Url;

use crate::utils::{clean_text, fetch_html};

/// Regex to extract the chapter number from a `chuong-NN/` URL fragment.
static CHUONG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"/chuong-(\d+)/?$").unwrap());

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
