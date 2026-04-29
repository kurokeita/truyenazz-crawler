use anyhow::{Context, Result, anyhow};
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::time::Duration;
use unicode_normalization::UnicodeNormalization;

/// Browser-style User-Agent header sent on every outbound HTTP request.
/// Matches the value used by the TypeScript port for parity.
pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0 Safari/537.36";

/// Lines starting with any of these phrases are treated as scraped boilerplate
/// and excluded from chapter content.
pub const NOISE_PREFIXES: &[&str] = &[
    "Bạn đang đọc truyện mới tại",
    "Nhấn Mở Bình Luận",
    "Tham gia group Facebook",
    "Các bạn thông cảm vì website có hiện quảng cáo",
    "Website hoạt động dưới Giấy phép",
];

/// Pre-compiled regex matching one-or-more whitespace characters.
static WS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

/// Pre-compiled regex matching runs of dashes and whitespace, used by [`slugify`].
static SLUG_COLLAPSE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[-\s]+").unwrap());

/// Decompose `s` to NFKD form and drop combining diacritical marks (U+0300..U+036F).
/// Used by both prefix-matching and slug generation so they ignore Vietnamese tones.
fn strip_combining_marks(s: &str) -> String {
    s.nfkd()
        .filter(|c| !matches!(*c as u32, 0x0300..=0x036F))
        .collect()
}

/// Lowercase, fold đ/Đ to `d`, strip diacritics, and collapse whitespace.
/// The result is the canonical form used to compare against [`NOISE_PREFIXES`].
fn normalize_for_prefix_match(text: &str) -> String {
    let stripped: String = strip_combining_marks(text)
        .chars()
        .map(|c| match c {
            'đ' | 'Đ' => 'd',
            other => other,
        })
        .collect();
    let lowered = stripped.to_lowercase();
    WS_RE.replace_all(&lowered, " ").trim().to_string()
}

/// Decode HTML entities, replace non-breaking spaces, collapse repeated
/// whitespace into single spaces, and trim. Mirrors the TS `cleanText` helper.
pub fn clean_text(text: &str) -> String {
    let decoded = html_escape::decode_html_entities(text);
    let nbsp_replaced = decoded.replace('\u{00a0}', " ");
    WS_RE.replace_all(&nbsp_replaced, " ").trim().to_string()
}

/// Returns true when `text` is empty or starts with one of the known
/// noise prefixes (ads, comment promos, copyright notices) under
/// diacritic-insensitive comparison.
pub fn is_noise(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let normalized = normalize_for_prefix_match(text);
    if normalized.is_empty() {
        return true;
    }
    NOISE_PREFIXES
        .iter()
        .any(|prefix| normalized.starts_with(&normalize_for_prefix_match(prefix)))
}

/// Generate a filesystem-safe ASCII slug from `text`, falling back to
/// `fallback` when nothing usable remains. Keeps at most the first 120
/// characters so chapter directory names never get pathologically long.
pub fn slugify(text: &str, fallback: &str) -> String {
    let stripped: String = strip_combining_marks(text)
        .chars()
        .filter(|c| c.is_ascii())
        .collect();
    let lowered = stripped.to_lowercase();
    let trimmed = lowered.trim();

    let allowed: String = trimmed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || c.is_whitespace() || *c == '-')
        .collect();

    let collapsed = SLUG_COLLAPSE_RE.replace_all(&allowed, "_").to_string();
    let truncated: String = collapsed.chars().take(120).collect();
    if truncated.is_empty() {
        fallback.to_string()
    } else {
        truncated
    }
}

/// Build the canonical chapter URL for a novel base URL and chapter number,
/// e.g. `https://truyenazz.me/foo` + `7` -> `https://truyenazz.me/foo/chuong-7/`.
/// Trailing slashes on the base URL are stripped before joining.
pub fn build_chapter_url(base_url: &str, chapter_number: u32) -> String {
    let trimmed = base_url.trim_end_matches('/');
    format!("{}/chuong-{}/", trimmed, chapter_number)
}

/// Construct a reqwest client preconfigured with our User-Agent and timeout.
fn http_client(timeout: Duration) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(timeout)
        .build()
        .context("failed to build HTTP client")
}

/// Fetch `url` and return the response body as a string with the default 30s timeout.
pub async fn fetch_html(url: &str) -> Result<String> {
    fetch_html_with_timeout(url, Duration::from_secs(30)).await
}

/// Fetch `url` and return the response body as a string, erroring on non-2xx
/// or transport failures. Caller controls the request timeout.
pub async fn fetch_html_with_timeout(url: &str, timeout: Duration) -> Result<String> {
    let client = http_client(timeout)?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {url}"))?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "HTTP {} while fetching {}",
            response.status().as_u16(),
            url
        ));
    }

    response
        .text()
        .await
        .with_context(|| format!("failed to read body from {url}"))
}

/// Bytes plus the parsed `Content-Type` (without parameters) returned by
/// [`download_binary`]. The crawler uses this to pick a cover image extension.
pub struct DownloadedBinary {
    /// Raw response body bytes.
    pub content: Vec<u8>,
    /// Just the media type (e.g. `image/jpeg`); charset and other params dropped.
    pub content_type: String,
}

/// Download a binary resource (e.g. cover image) using the default 30s timeout.
pub async fn download_binary(url: &str) -> Result<DownloadedBinary> {
    download_binary_with_timeout(url, Duration::from_secs(30)).await
}

/// Download a binary resource and return its bytes alongside the bare media
/// type. Errors on non-2xx responses or transport failures.
pub async fn download_binary_with_timeout(
    url: &str,
    timeout: Duration,
) -> Result<DownloadedBinary> {
    let client = http_client(timeout)?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {url}"))?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "HTTP {} while fetching {}",
            response.status().as_u16(),
            url
        ));
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|raw| raw.split(';').next().unwrap_or("").trim().to_string())
        .unwrap_or_default();

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read bytes from {url}"))?;

    Ok(DownloadedBinary {
        content: bytes.to_vec(),
        content_type,
    })
}

/// Async sleep for `seconds` (fractional). Returns immediately for non-positive
/// values so a delay of 0 or a negative does not yield an unwanted yield point.
pub async fn sleep_seconds(seconds: f64) {
    if seconds <= 0.0 {
        return;
    }
    let millis = (seconds * 1000.0) as u64;
    tokio::time::sleep(Duration::from_millis(millis)).await;
}

/// Recursively create `dir_path` if it does not exist; succeeds when the
/// directory already exists.
pub async fn ensure_dir(dir_path: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dir_path)
        .await
        .with_context(|| format!("failed to create directory {}", dir_path.display()))
}

/// Returns true if `file_path` resolves to an existing filesystem entry.
/// Permission errors and other I/O failures are reported as `false`.
pub async fn file_exists(file_path: &Path) -> bool {
    tokio::fs::try_exists(file_path).await.unwrap_or(false)
}

/// Locate the EPUB embedding font.
///
/// When `explicit_font_path` is supplied, that path must exist or this returns
/// an error. Otherwise the function scans a small list of well-known locations
/// (next to the executable, one directory up, and the current working
/// directory) for `Bokerlam.ttf`, returning `None` when nothing is found so
/// callers can fall back to a generic serif family.
pub async fn find_font_file(explicit_font_path: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(path) = explicit_font_path {
        let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if file_exists(&resolved).await {
            return Ok(Some(resolved));
        }
        return Err(anyhow!("Font file not found: {}", resolved.display()));
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|q| q.to_path_buf()));

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = &exe_dir {
        candidates.push(dir.join("Bokerlam.ttf"));
        if let Some(parent) = dir.parent() {
            candidates.push(parent.join("Bokerlam.ttf"));
        }
    }
    candidates.push(cwd.join("Bokerlam.ttf"));

    for candidate in candidates {
        if file_exists(&candidate).await {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}
