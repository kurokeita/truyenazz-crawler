use anyhow::{Context, Result, anyhow};
use regex::Regex;
use scraper::{Html, Selector};
use std::path::{Path, PathBuf};

use crate::utils::clean_text;

/// List the saved `chapter_NNNN.html` files in `chapter_dir` in numeric
/// order. Errors when the directory contains none.
pub async fn list_chapter_files(chapter_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = tokio::fs::read_dir(chapter_dir)
        .await
        .with_context(|| format!("failed to read directory {}", chapter_dir.display()))?;
    let pattern = Regex::new(r"^chapter_\d+\.html$").unwrap();
    let mut files: Vec<PathBuf> = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name_str = match name.to_str() {
            Some(s) => s,
            None => continue,
        };
        if pattern.is_match(name_str) {
            files.push(entry.path());
        }
    }
    if files.is_empty() {
        return Err(anyhow!(
            "No chapter_*.html files found in {}",
            chapter_dir.display()
        ));
    }
    files.sort();
    Ok(files)
}

/// Title and body fragment extracted from a previously saved chapter file.
#[derive(Debug, Clone)]
pub struct SavedChapter {
    /// Chapter title pulled from `.chapter-title` (or `<h1>` fallback).
    pub title: String,
    /// Inner HTML of the `.chapter-content` div as written by the crawler.
    pub body_html: String,
}

/// Read a saved chapter HTML file and return its title plus the inner HTML
/// of the chapter body. Errors when either selector is missing or empty.
pub async fn extract_title_and_body_from_saved_chapter(
    chapter_path: &Path,
) -> Result<SavedChapter> {
    let raw = tokio::fs::read_to_string(chapter_path)
        .await
        .with_context(|| format!("failed to read {}", chapter_path.display()))?;
    let doc = Html::parse_document(&raw);

    let title_sel = Selector::parse(".chapter-title").unwrap();
    let h1_sel = Selector::parse("h1").unwrap();
    let title_elem = doc
        .select(&title_sel)
        .next()
        .or_else(|| doc.select(&h1_sel).next());

    let body_sel = Selector::parse(".chapter-content").unwrap();
    let body_elem = doc.select(&body_sel).next();

    let (title_elem, body_elem) = match (title_elem, body_elem) {
        (Some(t), Some(b)) => (t, b),
        _ => {
            return Err(anyhow!(
                "Missing .chapter-title or .chapter-content in {}",
                chapter_path.display()
            ));
        }
    };

    let title = clean_text(&title_elem.text().collect::<String>());
    let body_html = body_elem.inner_html().trim().to_string();
    if body_html.is_empty() {
        return Err(anyhow!(
            "Empty .chapter-content in {}",
            chapter_path.display()
        ));
    }
    Ok(SavedChapter { title, body_html })
}
