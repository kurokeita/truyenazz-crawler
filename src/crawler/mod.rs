mod chapter;
mod discovery;
mod parser;
mod types;

pub use chapter::crawl_chapter;
pub use discovery::{discover_last_chapter_number, discover_last_chapter_number_from_html};
pub use parser::{
    ChapterContent, NON_CONTENT_ATTRS, build_html_document, escape_html, extract_full_chapter_text,
};
pub use types::{
    CrawlChapterParams, CrawlResult, CrawlStatus, ExistingChapterDecision, ExistingFilePolicy,
};
