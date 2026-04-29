use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};

use crate::utils::{
    build_chapter_url, ensure_dir, fetch_html, file_exists, sleep_seconds, slugify,
};

use super::parser::{build_html_document, extract_full_chapter_text};
use super::types::{
    CrawlChapterParams, CrawlResult, CrawlStatus, ExistingChapterDecision, ExistingFilePolicy,
};

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
