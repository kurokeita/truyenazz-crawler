use truyenazz_crawler::crawler::{
    build_html_document, discover_last_chapter_number, discover_last_chapter_number_from_html,
    escape_html, extract_full_chapter_text,
};

#[test]
fn escape_html_replaces_special_characters() {
    assert_eq!(
        escape_html("<a href=\"x\">'b' & c</a>"),
        "&lt;a href=&quot;x&quot;&gt;&apos;b&apos; &amp; c&lt;/a&gt;"
    );
}

#[test]
fn escape_html_preserves_safe_text() {
    assert_eq!(escape_html("plain text 1 2 3"), "plain text 1 2 3");
}

#[test]
fn extract_full_chapter_text_pulls_titles_and_paragraphs() {
    let html = r#"
<html><body>
  <div class="rv-full-story-title"><h1>Người Chồng Vô Dụng</h1></div>
  <div class="rv-chapt-title"><h2>Chương 12: Bí mật</h2></div>
  <div class="chapter-c">
    <p>Đoạn một.</p>
    <p>Đoạn hai.</p>
    <p>Bạn đang đọc truyện mới tại spam.com</p>
    <p>Đoạn ba.</p>
  </div>
</body></html>
"#;
    let chapter = extract_full_chapter_text(html).unwrap();
    assert_eq!(chapter.novel_title, "Người Chồng Vô Dụng");
    assert_eq!(chapter.chapter_title, "Chương 12: Bí mật");
    assert_eq!(
        chapter.paragraphs,
        vec!["Đoạn một.", "Đoạn hai.", "Đoạn ba."]
    );
}

#[test]
fn extract_full_chapter_text_falls_back_to_default_titles() {
    let html = r#"
<html><body>
  <div class="chapter-c">
    <p>Hello world.</p>
  </div>
</body></html>
"#;
    let chapter = extract_full_chapter_text(html).unwrap();
    assert_eq!(chapter.novel_title, "Unknown Novel");
    assert_eq!(chapter.chapter_title, "Untitled Chapter");
    assert_eq!(chapter.paragraphs, vec!["Hello world."]);
}

#[test]
fn extract_full_chapter_text_dedupes_consecutive_lines() {
    let html = r#"
<div class="chapter-c">
  <p>Lặp lại</p>
  <p>Lặp lại</p>
  <p>Khác</p>
</div>
"#;
    let chapter = extract_full_chapter_text(html).unwrap();
    assert_eq!(chapter.paragraphs, vec!["Lặp lại", "Khác"]);
}

#[test]
fn extract_full_chapter_text_extracts_injected_backup_content() {
    let injected = "var contentS = '<p>Đoạn ẩn 1.</p><p>Đoạn ẩn 2.</p>'; div.";
    let injected = format!("{}innerHTML = contentS;", injected);
    let html = format!(
        r#"
<html><body>
  <div class="chapter-c">
    <p>Mở đầu.</p>
    <div id="data-content-truyen-backup"></div>
    <p>Kết thúc.</p>
  </div>
  <script>{}</script>
</body></html>
"#,
        injected
    );
    let chapter = extract_full_chapter_text(&html).unwrap();
    assert_eq!(
        chapter.paragraphs,
        vec!["Mở đầu.", "Đoạn ẩn 1.", "Đoạn ẩn 2.", "Kết thúc."]
    );
}

#[test]
fn extract_full_chapter_text_errors_when_chapter_div_missing() {
    let html = "<html><body><p>nothing</p></body></html>";
    let err = extract_full_chapter_text(html).unwrap_err();
    assert!(err.to_string().contains("chapter-c"));
}

#[test]
fn build_html_document_escapes_titles_and_paragraphs() {
    let doc = build_html_document(
        "A & B",
        "<Chapter>",
        &["Hello & goodbye".to_string(), "<script>".to_string()],
    );
    assert!(doc.contains("<title>&lt;Chapter&gt;</title>"));
    assert!(doc.contains("<div class=\"novel-title\">A &amp; B</div>"));
    assert!(doc.contains("<p>Hello &amp; goodbye</p>"));
    assert!(doc.contains("<p>&lt;script&gt;</p>"));
}

#[test]
fn build_html_document_renders_chapter_title_as_h1() {
    let doc = build_html_document("N", "C1", &[]);
    assert!(doc.contains("<h1 class=\"chapter-title\">C1</h1>"));
}

#[tokio::test]
async fn discover_last_chapter_number_parses_latest_section() {
    let mut server = mockito::Server::new_async().await;
    let html = r#"
<html><body>
  <h3>Mục Lục</h3>
  <div><ul><li><a href="/foo/chuong-1/">c1</a></li></ul></div>
  <div>
    <h3>Chương Mới Nhất</h3>
  </div>
  <div>
    <ul>
      <li><a href="/foo/chuong-100/">c100</a></li>
      <li><a href="/foo/chuong-101/">c101</a></li>
      <li><a href="/foo/chuong-102/">c102</a></li>
    </ul>
  </div>
</body></html>
"#;
    let _mock = server
        .mock("GET", "/foo/")
        .with_status(200)
        .with_body(html)
        .create_async()
        .await;
    let last = discover_last_chapter_number(&format!("{}/foo", server.url()))
        .await
        .unwrap();
    assert_eq!(last, 102);
}

#[tokio::test]
async fn discover_last_chapter_number_errors_when_section_missing() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/foo/")
        .with_status(200)
        .with_body("<html></html>")
        .create_async()
        .await;
    let err = discover_last_chapter_number(&format!("{}/foo", server.url()))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Chương Mới Nhất"));
}

#[test]
fn discover_last_chapter_number_from_html_parses_pure_html() {
    let html = r#"
<html><body>
  <h3>Mục Lục</h3>
  <div><ul><li><a href="/foo/chuong-1/">c1</a></li></ul></div>
  <div>
    <h3>Chương Mới Nhất</h3>
  </div>
  <div>
    <ul>
      <li><a href="/foo/chuong-50/">c50</a></li>
      <li><a href="/foo/chuong-51/">c51</a></li>
    </ul>
  </div>
</body></html>
"#;
    let n = discover_last_chapter_number_from_html(html, "https://truyenazz.me/foo/").unwrap();
    assert_eq!(n, 51);
}

#[test]
fn discover_last_chapter_number_from_html_errors_on_missing_section() {
    let err =
        discover_last_chapter_number_from_html("<html></html>", "https://x/foo/").unwrap_err();
    assert!(err.to_string().contains("Chương Mới Nhất"));
}
