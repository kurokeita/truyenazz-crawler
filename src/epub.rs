use anyhow::{Context, Result, anyhow};
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{Html, Selector};
use std::io::Write;
use std::path::{Path, PathBuf};
use url::Url;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::crawler::escape_html;
use crate::font::{FontMetadata, extract_font_metadata};
use crate::utils::{clean_text, download_binary, fetch_html, file_exists, find_font_file, slugify};

/// Pre-compiled regex matching the trailing " - truyenazz" suffix on novel
/// page titles.
static TRUYENAZZ_SUFFIX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\s*-\s*truyenazz\s*$").unwrap());

/// Pre-compiled regex pulling the author name from the page body text.
static AUTHOR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)Tác giả:\s*([^\n\r]+)").unwrap());

/// XML escape — same as the crawler's `escape_html` and reused here for
/// clarity at the call sites.
fn escape_xml(text: &str) -> String {
    escape_html(text)
}

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

/// One entry in the EPUB chapter manifest used by the spine/nav/ncx/opf
/// builders.
#[derive(Debug, Clone)]
pub struct ChapterEntry {
    /// Manifest id (e.g. `chapter_0001`).
    pub id: String,
    /// File name relative to `EPUB/text/` (e.g. `chapter_0001.xhtml`).
    pub file_name: String,
    /// Display title for navigation.
    pub title: String,
}

/// Render the per-chapter XHTML used inside the EPUB.
pub fn chapter_xhtml(title: &str, body_html: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
<!DOCTYPE html>\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"vi\" lang=\"vi\">\n\
  <head>\n\
    <title>{title_esc}</title>\n\
    <link href=\"../styles/main.css\" rel=\"stylesheet\" type=\"text/css\"/>\n\
  </head>\n\
  <body>\n\
    <h1>{title_esc}</h1>\n\
    {body}\n\
  </body>\n\
</html>",
        title_esc = escape_xml(title),
        body = body_html,
    )
}

/// Render the title page XHTML, optionally including the author below the
/// novel title.
pub fn title_page_xhtml(title: &str, author: Option<&str>) -> String {
    let author_html = author
        .map(|a| {
            format!(
                "<p style=\"text-indent:0;text-align:center;\">{}</p>",
                escape_xml(a)
            )
        })
        .unwrap_or_default();
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
<!DOCTYPE html>\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"vi\" lang=\"vi\">\n\
  <head>\n\
    <title>{title_esc}</title>\n\
    <link href=\"../styles/main.css\" rel=\"stylesheet\" type=\"text/css\"/>\n\
  </head>\n\
  <body>\n\
    <h1>{title_esc}</h1>\n\
    {author_html}\n\
  </body>\n\
</html>",
        title_esc = escape_xml(title),
        author_html = author_html,
    )
}

/// Render the EPUB navigation document (`nav.xhtml`) — every chapter as a
/// link.
pub fn nav_xhtml(novel_title: &str, chapters: &[ChapterEntry]) -> String {
    let items = chapters
        .iter()
        .map(|c| {
            format!(
                "        <li><a href=\"text/{}\">{}</a></li>",
                c.file_name,
                escape_xml(&c.title)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
<!DOCTYPE html>\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"vi\" lang=\"vi\">\n\
  <head>\n\
    <title>{title_esc}</title>\n\
    <link href=\"styles/main.css\" rel=\"stylesheet\" type=\"text/css\"/>\n\
  </head>\n\
  <body>\n\
    <nav epub:type=\"toc\" id=\"toc\">\n\
      <h1>Mục lục</h1>\n\
      <ol>\n\
{items}\n\
      </ol>\n\
    </nav>\n\
  </body>\n\
</html>",
        title_esc = escape_xml(novel_title),
        items = items,
    )
}

/// Render the legacy NCX table of contents (`toc.ncx`).
pub fn ncx_xml(novel_title: &str, identifier: &str, chapters: &[ChapterEntry]) -> String {
    let nav_points = chapters
        .iter()
        .enumerate()
        .map(|(index, chapter)| {
            format!(
                "    <navPoint id=\"navPoint-{n}\" playOrder=\"{n}\">\n\
      <navLabel><text>{title}</text></navLabel>\n\
      <content src=\"text/{file}\"/>\n\
    </navPoint>",
                n = index + 1,
                title = escape_xml(&chapter.title),
                file = chapter.file_name,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n\
  <head>\n\
    <meta name=\"dtb:uid\" content=\"{identifier_esc}\"/>\n\
    <meta name=\"dtb:depth\" content=\"1\"/>\n\
    <meta name=\"dtb:totalPageCount\" content=\"0\"/>\n\
    <meta name=\"dtb:maxPageNumber\" content=\"0\"/>\n\
  </head>\n\
  <docTitle><text>{title_esc}</text></docTitle>\n\
  <navMap>\n\
{nav_points}\n\
  </navMap>\n\
</ncx>",
        identifier_esc = escape_xml(identifier),
        title_esc = escape_xml(novel_title),
        nav_points = nav_points,
    )
}

/// All inputs needed to render the EPUB package document (`content.opf`).
pub struct ContentOpfParams {
    /// Stable identifier for the EPUB (we use the novel main URL).
    pub identifier: String,
    /// Display title.
    pub title: String,
    /// Optional author/creator name.
    pub author: Option<String>,
    /// Whether to include the cover image manifest entry.
    pub include_cover: bool,
    /// Cover image extension including the dot (e.g. ".jpg").
    pub cover_ext: String,
    /// Whether to include the embedded font manifest entry.
    pub include_font: bool,
    /// Embedded font file name (relative to `EPUB/fonts/`).
    pub font_file_name: String,
    /// Per-chapter manifest + spine entries.
    pub chapters: Vec<ChapterEntry>,
}

/// Render the EPUB 3 package document.
pub fn content_opf(params: ContentOpfParams) -> String {
    let author_metadata = params
        .author
        .as_ref()
        .map(|a| format!("    <dc:creator>{}</dc:creator>\n", escape_xml(a)))
        .unwrap_or_default();
    let cover_meta = if params.include_cover {
        "    <meta name=\"cover\" content=\"cover-image\"/>\n".to_string()
    } else {
        String::new()
    };
    let cover_manifest = if params.include_cover {
        let media_type = mime_guess::from_ext(params.cover_ext.trim_start_matches('.'))
            .first_raw()
            .unwrap_or("image/jpeg")
            .to_string();
        format!(
            "    <item id=\"cover-image\" href=\"cover{}\" media-type=\"{}\"/>\n",
            params.cover_ext, media_type
        )
    } else {
        String::new()
    };
    let font_manifest = if params.include_font {
        let media_type = mime_guess::from_path(&params.font_file_name)
            .first_raw()
            .unwrap_or("font/ttf")
            .to_string();
        format!(
            "    <item id=\"epub-font\" href=\"fonts/{}\" media-type=\"{}\"/>\n",
            params.font_file_name, media_type
        )
    } else {
        String::new()
    };
    let chapter_manifest = params
        .chapters
        .iter()
        .map(|c| {
            format!(
                "    <item id=\"{}\" href=\"text/{}\" media-type=\"application/xhtml+xml\"/>",
                c.id, c.file_name
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let spine_items = params
        .chapters
        .iter()
        .map(|c| format!("    <itemref idref=\"{}\"/>", c.id))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"BookId\">\n\
  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n\
    <dc:identifier id=\"BookId\">{ident}</dc:identifier>\n\
    <dc:title>{title}</dc:title>\n\
    <dc:language>vi</dc:language>\n\
{author}{cover_meta}  </metadata>\n\
  <manifest>\n\
    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n\
    <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n\
    <item id=\"style\" href=\"styles/main.css\" media-type=\"text/css\"/>\n\
    <item id=\"titlepage\" href=\"text/titlepage.xhtml\" media-type=\"application/xhtml+xml\"/>\n\
{cover_manifest}{font_manifest}{chapter_manifest}\n\
  </manifest>\n\
  <spine toc=\"ncx\">\n\
    <itemref idref=\"nav\"/>\n\
    <itemref idref=\"titlepage\"/>\n\
{spine_items}\n\
  </spine>\n\
</package>",
        ident = escape_xml(&params.identifier),
        title = escape_xml(&params.title),
        author = author_metadata,
        cover_meta = cover_meta,
        cover_manifest = cover_manifest,
        font_manifest = font_manifest,
        chapter_manifest = chapter_manifest,
        spine_items = spine_items,
    )
}

/// Inputs to [`build_epub`].
pub struct BuildEpubParams {
    /// Main novel page URL (used for metadata + identifier + cover lookup).
    pub novel_main_url: String,
    /// Directory containing the saved chapter HTML files.
    pub chapter_dir: PathBuf,
    /// Optional explicit output path; defaults to `<chapter_dir>/<slug>.epub`.
    pub output_epub: Option<PathBuf>,
    /// Optional override for the embedded font.
    pub font_path: Option<PathBuf>,
}

/// Build the embedded stylesheet referencing the embedded font face.
fn build_main_css(font_metadata: &FontMetadata, font_file_name: &str) -> String {
    let escaped_family = font_metadata.family_name.replace('\'', "\\'");
    format!(
        "@font-face {{\n  font-family: '{family}';\n  src: url('../fonts/{font}');\n}}\n\n\
body {{\n  font-family: '{family}', serif;\n  line-height: 1.8;\n  margin: 0%;\n  padding: 0;\n}}\n\n\
h1 {{\n  text-align: center;\n  font-size: 2.2em;\n  font-weight: bold;\n  margin: 2.5em 0 1.5em 0;\n  padding: 0;\n}}\n\n\
p {{\n  margin: 0 0 0.9em 0;\n  text-indent: 2em;\n  text-align: justify;\n}}",
        family = escaped_family,
        font = font_file_name,
    )
}

/// Assemble the EPUB archive at `output_epub` from previously-saved chapter
/// HTML files in `chapter_dir`. Fetches the novel main page for metadata +
/// cover, embeds the font when available, and returns the final output path.
pub async fn build_epub(params: BuildEpubParams) -> Result<PathBuf> {
    let chapter_dir =
        std::fs::canonicalize(&params.chapter_dir).unwrap_or_else(|_| params.chapter_dir.clone());
    if !file_exists(&chapter_dir).await {
        return Err(anyhow!(
            "Chapter directory not found: {}",
            chapter_dir.display()
        ));
    }

    let main_html = fetch_html(&params.novel_main_url).await?;
    let novel_title = extract_novel_title_from_main_page(&main_html);
    let author = extract_author_from_main_page(&main_html);

    let cover_url = extract_cover_image_url(&params.novel_main_url, &main_html);
    let mut cover_bytes: Option<Vec<u8>> = None;
    let mut cover_ext = ".jpg".to_string();
    let mut cover_media_type = "image/jpeg".to_string();
    if let Some(url) = &cover_url
        && let Ok(downloaded) = download_binary(url).await
    {
        cover_bytes = Some(downloaded.content);
        if !downloaded.content_type.is_empty() {
            cover_media_type = downloaded.content_type.clone();
        }
        cover_ext = pick_cover_extension(url, &cover_media_type);
    }

    let output_epub = params
        .output_epub
        .clone()
        .unwrap_or_else(|| chapter_dir.join(format!("{}.epub", slugify(&novel_title, "book"))));

    let font_path = find_font_file(params.font_path.as_deref()).await?;
    let font_bytes = match &font_path {
        Some(path) => Some(tokio::fs::read(path).await?),
        None => None,
    };
    let font_metadata = match &font_path {
        Some(path) => extract_font_metadata(path).await?,
        None => FontMetadata {
            family_name: "serif".into(),
            extension: ".ttf".into(),
        },
    };
    let embedded_font_file_name = format!("epub-font{}", font_metadata.extension);

    if let Some(parent) = output_epub.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let chapter_files = list_chapter_files(&chapter_dir).await?;
    let mut chapter_entries: Vec<ChapterEntry> = Vec::with_capacity(chapter_files.len());
    let mut chapter_xhtmls: Vec<(String, String)> = Vec::with_capacity(chapter_files.len());
    for (index, chapter_file) in chapter_files.iter().enumerate() {
        let saved = extract_title_and_body_from_saved_chapter(chapter_file).await?;
        let file_name = format!("chapter_{:04}.xhtml", index + 1);
        let id = format!("chapter_{:04}", index + 1);
        chapter_xhtmls.push((
            file_name.clone(),
            chapter_xhtml(&saved.title, &saved.body_html),
        ));
        chapter_entries.push(ChapterEntry {
            id,
            file_name,
            title: saved.title,
        });
    }

    let css = build_main_css(&font_metadata, &embedded_font_file_name);
    let opf = content_opf(ContentOpfParams {
        identifier: params.novel_main_url.clone(),
        title: novel_title.clone(),
        author: author.clone(),
        include_cover: cover_bytes.is_some(),
        cover_ext: cover_ext.clone(),
        include_font: font_bytes.is_some(),
        font_file_name: embedded_font_file_name.clone(),
        chapters: chapter_entries.clone(),
    });
    let nav = nav_xhtml(&novel_title, &chapter_entries);
    let ncx = ncx_xml(&novel_title, &params.novel_main_url, &chapter_entries);
    let titlepage = title_page_xhtml(&novel_title, author.as_deref());

    let output_epub_clone = output_epub.clone();
    let cover_bytes_clone = cover_bytes;
    let cover_ext_clone = cover_ext;
    let font_bytes_clone = font_bytes;
    let embedded_font_file_name_clone = embedded_font_file_name;

    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::create(&output_epub_clone)
            .with_context(|| format!("failed to create {}", output_epub_clone.display()))?;
        let mut zip = zip::ZipWriter::new(file);

        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("mimetype", stored)?;
        zip.write_all(b"application/epub+zip")?;

        let deflated =
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("META-INF/container.xml", deflated)?;
        zip.write_all(b"<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n  <rootfiles>\n    <rootfile full-path=\"EPUB/content.opf\" media-type=\"application/oebps-package+xml\"/>\n  </rootfiles>\n</container>")?;

        zip.start_file("EPUB/styles/main.css", deflated)?;
        zip.write_all(css.as_bytes())?;

        if let Some(bytes) = &font_bytes_clone {
            zip.start_file(format!("EPUB/fonts/{}", embedded_font_file_name_clone), deflated)?;
            zip.write_all(bytes)?;
        }

        if let Some(bytes) = &cover_bytes_clone {
            zip.start_file(format!("EPUB/cover{}", cover_ext_clone), deflated)?;
            zip.write_all(bytes)?;
        }

        for (file_name, body) in &chapter_xhtmls {
            zip.start_file(format!("EPUB/text/{}", file_name), deflated)?;
            zip.write_all(body.as_bytes())?;
        }

        zip.start_file("EPUB/text/titlepage.xhtml", deflated)?;
        zip.write_all(titlepage.as_bytes())?;

        zip.start_file("EPUB/nav.xhtml", deflated)?;
        zip.write_all(nav.as_bytes())?;

        zip.start_file("EPUB/toc.ncx", deflated)?;
        zip.write_all(ncx.as_bytes())?;

        zip.start_file("EPUB/content.opf", deflated)?;
        zip.write_all(opf.as_bytes())?;

        zip.finish()?;
        Ok(())
    })
    .await
    .map_err(|e| anyhow!("epub writer task panicked: {}", e))??;

    Ok(output_epub)
}
