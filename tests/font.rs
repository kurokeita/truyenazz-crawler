use std::path::PathBuf;
use truyenazz_crawler::font::extract_font_metadata;

/// Path to the bundled Bokerlam.ttf used for the EPUB embed in production.
fn bundled_font_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Bokerlam.ttf")
}

#[tokio::test]
async fn extract_font_metadata_reads_family_name_from_bundled_font() {
    let path = bundled_font_path();
    assert!(path.exists(), "bundled font missing: {}", path.display());
    let metadata = extract_font_metadata(&path).await.unwrap();
    assert!(
        !metadata.family_name.is_empty(),
        "family name should not be empty"
    );
    assert_eq!(metadata.extension, ".ttf");
}

#[tokio::test]
async fn extract_font_metadata_falls_back_to_filename_for_invalid_buffer() {
    let dir = tempfile::tempdir().unwrap();
    // Buffer is larger than 12 bytes so we hit the table-walk branch, but
    // the table count points nowhere useful. We expect the function to
    // gracefully fall back to using the file stem as the family name.
    let path = dir.path().join("FakeFamily.ttf");
    let mut buf = vec![0u8; 64];
    buf[0..4].copy_from_slice(b"\x00\x01\x00\x00");
    buf[4] = 0x00;
    buf[5] = 0x00; // numTables = 0
    tokio::fs::write(&path, &buf).await.unwrap();

    let metadata = extract_font_metadata(&path).await.unwrap();
    assert_eq!(metadata.family_name, "FakeFamily");
    assert_eq!(metadata.extension, ".ttf");
}

#[tokio::test]
async fn extract_font_metadata_errors_for_buffer_smaller_than_header() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tiny.ttf");
    tokio::fs::write(&path, b"abc").await.unwrap();
    let err = extract_font_metadata(&path).await.unwrap_err();
    assert!(err.to_string().contains("Invalid font file"));
}

#[tokio::test]
async fn extract_font_metadata_uses_lowercase_extension() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Mixed.TTF");
    let mut buf = vec![0u8; 64];
    buf[0..4].copy_from_slice(b"\x00\x01\x00\x00");
    tokio::fs::write(&path, &buf).await.unwrap();
    let metadata = extract_font_metadata(&path).await.unwrap();
    assert_eq!(metadata.extension, ".ttf");
}
