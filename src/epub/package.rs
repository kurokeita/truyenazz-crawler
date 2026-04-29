use crate::crawler::escape_html;

/// XML escape — same as the crawler's `escape_html` and reused here for
/// clarity at the call sites.
fn escape_xml(text: &str) -> String {
    escape_html(text)
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
