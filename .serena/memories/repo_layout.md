# Repo layout

``` shell
truyenazz-crawler/
├── Cargo.toml                 # single crate (library + bin)
├── Cargo.lock
├── Bokerlam.ttf               # bundled font for EPUB embedding
├── .gitignore                 # ignores node_modules, target/, output/
├── src/
│   ├── lib.rs                 # declares cli, crawler, epub, font, runner, ui, utils
│   ├── utils.rs               # text + http + fs helpers
│   ├── font.rs                # TTF name-table parser
│   ├── crawler.rs             # HTML parse, build_html_document, crawl_chapter, discover_last_chapter_number
│   ├── epub.rs                # build_epub, ChapterEntry, ContentOpfParams
│   ├── runner.rs              # sequential + parallel runners, ProgressEvent
│   ├── cli.rs                 # clap derive RawArgs, CliOptions, parse_from
│   ├── ui.rs                  # ratatui TUI + DownloadProgress + run_download_screen
│   └── bin/
│       └── truyenazz-crawl.rs # main, dispatches interactive vs CLI
├── tests/
│   ├── utils.rs               # 23 tests (text + http + fs helpers)
│   ├── font.rs                # 4 tests (name-table parsing + fallback)
│   ├── crawler.rs             # 11 tests (extraction, dedup, discover_last_chapter_number)
│   ├── crawl_chapter.rs       # 7 tests (existing-file policy state machine)
│   ├── epub.rs                # 21 tests (xhtml/ncx/opf + zip integration)
│   ├── runner.rs              # 7 tests (sequential, parallel, progress events)
│   ├── cli.rs                 # 12 tests (clap parsing + validators)
│   └── ui.rs                  # 13 tests (text input, select, DownloadProgress)
├── typescript/                # ORIGINAL TS port — kept for reference, will be deleted
│   └── src/                   # types.ts utils.ts crawler.ts epub.ts font.ts cli.ts ui.ts internal-cli-helpers.ts
├── target/                    # cargo build output (gitignored)
├── output/                    # default chapter output dir (gitignored)
└── .claude/
    └── skills/
        └── rust-testing/SKILL.md   # project-local TDD reference
```

## Auto-memory pointers (separate from serena memories)

`/Users/minhle/.claude/projects/-Users-minhle-dev-truyenazz-crawler/memory/`
holds the harness's own auto-memory. The current entry:

- `feedback_doc_comments.md` — every fn in this repo gets a doc comment.
This is a user-confirmed override of the default "no comments" rule.
