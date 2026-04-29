use std::time::Duration;
use truyenazz_crawler::utils::{
    build_chapter_url, clean_text, download_binary, ensure_dir, fetch_html, file_exists,
    find_font_file, is_noise, sleep_seconds, slugify,
};

#[test]
fn clean_text_decodes_html_entities() {
    assert_eq!(clean_text("Hello&nbsp;world"), "Hello world");
    assert_eq!(clean_text("a &amp; b"), "a & b");
}

#[test]
fn clean_text_collapses_whitespace_and_trims() {
    assert_eq!(clean_text("  hello   world  "), "hello world");
    assert_eq!(clean_text("line1\n\n\tline2"), "line1 line2");
}

#[test]
fn clean_text_replaces_nbsp_codepoint() {
    assert_eq!(clean_text("hello\u{00a0}world"), "hello world");
}

#[test]
fn clean_text_handles_empty_string() {
    assert_eq!(clean_text(""), "");
    assert_eq!(clean_text("   "), "");
}

#[test]
fn is_noise_returns_true_for_empty() {
    assert!(is_noise(""));
}

#[test]
fn is_noise_matches_known_prefixes() {
    assert!(is_noise("Bạn đang đọc truyện mới tại truyenazz.me"));
    assert!(is_noise("Nhấn Mở Bình Luận để bình luận"));
    assert!(is_noise("Tham gia group Facebook ngay"));
}

#[test]
fn is_noise_is_diacritic_insensitive() {
    assert!(is_noise("ban dang doc truyen moi tai abc"));
    assert!(is_noise("BẠN ĐANG ĐỌC TRUYỆN MỚI TẠI"));
}

#[test]
fn is_noise_returns_false_for_real_content() {
    assert!(!is_noise("Chương 1: Khởi đầu"));
    assert!(!is_noise("Hello world"));
}

#[test]
fn slugify_lowercases_and_underscores() {
    assert_eq!(slugify("Hello World", "novel"), "hello_world");
}

#[test]
fn slugify_strips_diacritics_and_non_ascii() {
    assert_eq!(
        slugify("Người Chồng Vô Dụng", "novel"),
        "nguoi_chong_vo_dung"
    );
}

#[test]
fn slugify_uses_fallback_when_empty() {
    assert_eq!(slugify("", "book"), "book");
    assert_eq!(slugify("???", "book"), "book");
}

#[test]
fn slugify_truncates_to_120_chars() {
    let long = "a".repeat(200);
    let s = slugify(&long, "novel");
    assert_eq!(s.len(), 120);
}

#[test]
fn slugify_preserves_dashes_as_underscore() {
    assert_eq!(slugify("foo-bar baz", "novel"), "foo_bar_baz");
}

#[test]
fn build_chapter_url_appends_chapter_segment() {
    assert_eq!(
        build_chapter_url("https://truyenazz.me/foo", 7),
        "https://truyenazz.me/foo/chuong-7/"
    );
}

#[test]
fn build_chapter_url_strips_trailing_slashes() {
    assert_eq!(
        build_chapter_url("https://truyenazz.me/foo/", 1),
        "https://truyenazz.me/foo/chuong-1/"
    );
    assert_eq!(
        build_chapter_url("https://truyenazz.me/foo///", 42),
        "https://truyenazz.me/foo/chuong-42/"
    );
}

#[tokio::test]
async fn file_exists_returns_true_for_existing_file_and_false_otherwise() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hello.txt");
    assert!(!file_exists(&path).await);
    tokio::fs::write(&path, b"hi").await.unwrap();
    assert!(file_exists(&path).await);
}

#[tokio::test]
async fn ensure_dir_creates_nested_directories() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a/b/c");
    ensure_dir(&nested).await.unwrap();
    assert!(nested.is_dir());
    ensure_dir(&nested).await.unwrap();
}

#[tokio::test]
async fn sleep_seconds_returns_immediately_for_non_positive() {
    let start = std::time::Instant::now();
    sleep_seconds(0.0).await;
    sleep_seconds(-1.0).await;
    assert!(start.elapsed() < Duration::from_millis(50));
}

#[tokio::test]
async fn find_font_file_returns_explicit_path_when_provided() {
    let dir = tempfile::tempdir().unwrap();
    let font = dir.path().join("MyFont.ttf");
    tokio::fs::write(&font, b"fake").await.unwrap();
    let result = find_font_file(Some(&font)).await.unwrap();
    assert!(result.is_some());
    assert_eq!(
        result.unwrap().file_name().unwrap(),
        std::ffi::OsStr::new("MyFont.ttf")
    );
}

#[tokio::test]
async fn find_font_file_errors_when_explicit_path_missing() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist.ttf");
    let result = find_font_file(Some(&missing)).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn fetch_html_returns_body_for_2xx() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/page")
        .with_status(200)
        .with_body("<html>ok</html>")
        .create_async()
        .await;
    let body = fetch_html(&format!("{}/page", server.url())).await.unwrap();
    assert!(body.contains("ok"));
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_html_errors_on_non_2xx() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/missing")
        .with_status(404)
        .create_async()
        .await;
    let err = fetch_html(&format!("{}/missing", server.url()))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("HTTP 404"));
}

#[tokio::test]
async fn download_binary_returns_bytes_and_content_type() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/img.jpg")
        .with_status(200)
        .with_header("content-type", "image/jpeg; charset=utf-8")
        .with_body(&[0xFF, 0xD8, 0xFF][..])
        .create_async()
        .await;
    let result = download_binary(&format!("{}/img.jpg", server.url()))
        .await
        .unwrap();
    assert_eq!(result.content, vec![0xFF, 0xD8, 0xFF]);
    assert_eq!(result.content_type, "image/jpeg");
}
