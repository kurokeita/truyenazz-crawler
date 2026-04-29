mod build;
mod chapters;
mod metadata;
mod package;

pub use build::{BuildEpubParams, build_epub};
pub use chapters::{SavedChapter, extract_title_and_body_from_saved_chapter, list_chapter_files};
pub use metadata::{
    extract_author_from_main_page, extract_cover_image_url,
    extract_novel_description_from_main_page, extract_novel_status_from_main_page,
    extract_novel_title_from_main_page, pick_cover_extension,
};
pub use package::{
    ChapterEntry, ContentOpfParams, chapter_xhtml, content_opf, nav_xhtml, ncx_xml,
    title_page_xhtml,
};
