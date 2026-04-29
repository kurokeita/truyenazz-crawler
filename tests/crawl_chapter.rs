use std::sync::Arc;
use truyenazz_crawler::crawler::{
    CrawlChapterParams, CrawlStatus, ExistingChapterDecision, ExistingFilePolicy, crawl_chapter,
};

/// Minimal HTML body returned by the mock origin server in these tests.
fn fake_chapter_html(novel: &str, chapter: &str, body: &str) -> String {
    format!(
        r#"<html><body>
  <div class="rv-full-story-title"><h1>{novel}</h1></div>
  <div class="rv-chapt-title"><h2>{chapter}</h2></div>
  <div class="chapter-c"><p>{body}</p></div>
</body></html>"#
    )
}

#[tokio::test]
async fn crawl_chapter_writes_html_when_file_missing() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/foo/chuong-1/")
        .with_status(200)
        .with_body(fake_chapter_html("Truyện X", "Chương 1", "Nội dung."))
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let prompt = Arc::new(|_: &std::path::Path| ExistingChapterDecision::Skip);
    let result = crawl_chapter(CrawlChapterParams {
        base_url: &format!("{}/foo", server.url()),
        chapter_number: 1,
        output_root: dir.path(),
        if_exists: ExistingFilePolicy::Ask,
        existing_policy: ExistingFilePolicy::Ask,
        delay: 0.0,
        novel_title: None,
        fast_skip: false,
        prompt,
    })
    .await
    .unwrap();
    assert_eq!(result.status, CrawlStatus::Written);
    assert_eq!(result.novel_title, "Truyện X");
    assert!(result.output_path.exists());
    let written = tokio::fs::read_to_string(&result.output_path)
        .await
        .unwrap();
    assert!(written.contains("Nội dung."));
    assert!(written.contains("<h1 class=\"chapter-title\">Chương 1</h1>"));
    assert_eq!(
        result.output_path.file_name().unwrap(),
        std::ffi::OsStr::new("chapter_0001.html")
    );
}

#[tokio::test]
async fn crawl_chapter_skips_when_file_exists_and_policy_is_skip() {
    let mut server = mockito::Server::new_async().await;
    // Even though we mock the URL, fast-skip should hit it. With policy=Skip + non-fast-skip,
    // we still fetch HTML but the save step returns Skipped. Provide the body anyway.
    let _mock = server
        .mock("GET", "/foo/chuong-2/")
        .with_status(200)
        .with_body(fake_chapter_html("Truyện X", "Chương 2", "Tươi mới."))
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    // Pre-create the destination file with stale content.
    let novel_dir = dir.path().join("truyen_x");
    tokio::fs::create_dir_all(&novel_dir).await.unwrap();
    let target = novel_dir.join("chapter_0002.html");
    tokio::fs::write(&target, b"old content").await.unwrap();

    let prompt = Arc::new(|_: &std::path::Path| ExistingChapterDecision::Skip);
    let result = crawl_chapter(CrawlChapterParams {
        base_url: &format!("{}/foo", server.url()),
        chapter_number: 2,
        output_root: dir.path(),
        if_exists: ExistingFilePolicy::Skip,
        existing_policy: ExistingFilePolicy::Ask,
        delay: 0.0,
        novel_title: None,
        fast_skip: false,
        prompt,
    })
    .await
    .unwrap();
    assert_eq!(result.status, CrawlStatus::Skipped);
    let preserved = tokio::fs::read_to_string(&target).await.unwrap();
    assert_eq!(preserved, "old content");
}

#[tokio::test]
async fn crawl_chapter_overwrites_when_policy_is_overwrite() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/foo/chuong-3/")
        .with_status(200)
        .with_body(fake_chapter_html("Truyện X", "Chương 3", "Mới."))
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let novel_dir = dir.path().join("truyen_x");
    tokio::fs::create_dir_all(&novel_dir).await.unwrap();
    let target = novel_dir.join("chapter_0003.html");
    tokio::fs::write(&target, b"stale").await.unwrap();

    let prompt = Arc::new(|_: &std::path::Path| ExistingChapterDecision::Skip);
    let result = crawl_chapter(CrawlChapterParams {
        base_url: &format!("{}/foo", server.url()),
        chapter_number: 3,
        output_root: dir.path(),
        if_exists: ExistingFilePolicy::Overwrite,
        existing_policy: ExistingFilePolicy::Ask,
        delay: 0.0,
        novel_title: None,
        fast_skip: false,
        prompt,
    })
    .await
    .unwrap();
    assert_eq!(result.status, CrawlStatus::Written);
    let written = tokio::fs::read_to_string(&result.output_path)
        .await
        .unwrap();
    assert!(written.contains("Mới."));
}

#[tokio::test]
async fn fast_skip_short_circuits_without_hitting_remote() {
    // No mocks registered: any remote fetch would fail with connection refused.
    let server = mockito::Server::new_async().await;
    let dir = tempfile::tempdir().unwrap();
    let novel_dir = dir.path().join("truyen_x");
    tokio::fs::create_dir_all(&novel_dir).await.unwrap();
    let target = novel_dir.join("chapter_0007.html");
    tokio::fs::write(&target, b"already on disk").await.unwrap();

    let prompt = Arc::new(|_: &std::path::Path| ExistingChapterDecision::Skip);
    let result = crawl_chapter(CrawlChapterParams {
        base_url: &format!("{}/foo", server.url()),
        chapter_number: 7,
        output_root: dir.path(),
        if_exists: ExistingFilePolicy::Skip,
        existing_policy: ExistingFilePolicy::Ask,
        delay: 0.0,
        novel_title: Some("Truyện X"),
        fast_skip: true,
        prompt,
    })
    .await
    .unwrap();
    assert_eq!(result.status, CrawlStatus::Skipped);
    assert_eq!(result.novel_title, "Truyện X");
}

#[tokio::test]
async fn skip_all_policy_short_circuits_remaining_chapters() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/foo/chuong-4/")
        .with_status(200)
        .with_body(fake_chapter_html("Truyện X", "Chương 4", "Bốn"))
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let novel_dir = dir.path().join("truyen_x");
    tokio::fs::create_dir_all(&novel_dir).await.unwrap();
    let target = novel_dir.join("chapter_0004.html");
    tokio::fs::write(&target, b"existing").await.unwrap();

    let prompt = Arc::new(|_: &std::path::Path| ExistingChapterDecision::Skip);
    let result = crawl_chapter(CrawlChapterParams {
        base_url: &format!("{}/foo", server.url()),
        chapter_number: 4,
        output_root: dir.path(),
        if_exists: ExistingFilePolicy::Ask,
        existing_policy: ExistingFilePolicy::SkipAll,
        delay: 0.0,
        novel_title: None,
        fast_skip: false,
        prompt,
    })
    .await
    .unwrap();
    assert_eq!(result.status, CrawlStatus::Skipped);
}

#[tokio::test]
async fn ask_policy_invokes_prompt_and_propagates_skip_all() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/foo/chuong-5/")
        .with_status(200)
        .with_body(fake_chapter_html("Truyện X", "Chương 5", "Năm"))
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let novel_dir = dir.path().join("truyen_x");
    tokio::fs::create_dir_all(&novel_dir).await.unwrap();
    let target = novel_dir.join("chapter_0005.html");
    tokio::fs::write(&target, b"keep me").await.unwrap();

    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let prompt = Arc::new(move |_: &std::path::Path| {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        ExistingChapterDecision::SkipAll
    });

    let result = crawl_chapter(CrawlChapterParams {
        base_url: &format!("{}/foo", server.url()),
        chapter_number: 5,
        output_root: dir.path(),
        if_exists: ExistingFilePolicy::Ask,
        existing_policy: ExistingFilePolicy::Ask,
        delay: 0.0,
        novel_title: None,
        fast_skip: false,
        prompt,
    })
    .await
    .unwrap();
    assert_eq!(result.status, CrawlStatus::SkipAll);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn crawl_chapter_errors_when_no_paragraphs_extracted() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/foo/chuong-6/")
        .with_status(200)
        .with_body("<html><body><div class=\"chapter-c\"></div></body></html>")
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let prompt = Arc::new(|_: &std::path::Path| ExistingChapterDecision::Skip);
    let err = crawl_chapter(CrawlChapterParams {
        base_url: &format!("{}/foo", server.url()),
        chapter_number: 6,
        output_root: dir.path(),
        if_exists: ExistingFilePolicy::Skip,
        existing_policy: ExistingFilePolicy::Ask,
        delay: 0.0,
        novel_title: None,
        fast_skip: false,
        prompt,
    })
    .await
    .unwrap_err();
    assert!(err.to_string().contains("No chapter content"));
}
