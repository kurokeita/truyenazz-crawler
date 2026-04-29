# Repo layout

```shell
truyenazz-crawler/
├── Cargo.toml                 # single Rust crate: library + truyenazz-crawl binary
├── Cargo.lock
├── Bokerlam.ttf               # bundled font used as the default EPUB font
├── LICENSE                    # GPL-3.0 license text
├── README.md                  # usage, architecture flow, CI/release notes
├── .github/
│   └── workflows/
│       ├── ci.yml             # PR/main CI: tests first, then platform builds
│       └── release.yml        # v* tag/manual release artifact builds
├── src/
│   ├── lib.rs                 # declares cli, crawler, epub, font, runner, ui, utils
│   ├── cli.rs                 # clap RawArgs, CliOptions, parsing, validation helpers
│   ├── runner.rs              # sequential/parallel chapter runners and ProgressEvent
│   ├── utils.rs               # URL, HTTP, text-cleaning, slug, filesystem helpers
│   ├── font.rs                # TTF/OTF metadata extraction for EPUB embedding
│   ├── bin/
│   │   └── truyenazz-crawl.rs # process entry point and top-level orchestration
│   ├── crawler/
│   │   ├── mod.rs             # public re-exports preserving truyenazz_crawler::crawler::*
│   │   ├── parser.rs          # chapter HTML parsing, escaping, saved chapter HTML rendering
│   │   ├── discovery.rs       # latest-chapter discovery from main-page HTML / URL
│   │   ├── types.rs           # crawl policies, decisions, statuses, params, result types
│   │   └── chapter.rs         # crawl_chapter save/skip/overwrite flow
│   ├── epub/
│   │   ├── mod.rs             # public re-exports preserving truyenazz_crawler::epub::*
│   │   ├── metadata.rs        # title/status/description/author/cover extraction
│   │   ├── chapters.rs        # saved chapter file listing and body extraction
│   │   ├── package.rs         # XHTML, nav, NCX, OPF generation helpers
│   │   └── build.rs           # build_epub orchestration and ZIP writing
│   └── ui/
│       ├── mod.rs             # TerminalGuard, palette/helpers, public ui::* re-exports
│       ├── plan.rs            # CrawlMode, InteractivePlan, SummaryParams, build_summary
│       ├── screens/
│       │   ├── mod.rs         # screen re-exports
│       │   ├── prompts.rs     # text/path/select/confirm/note screens
│       │   ├── loading.rs     # spinner screen for async tasks
│       │   └── download.rs    # TUI download progress screen
│       ├── widgets/
│       │   ├── mod.rs         # widget re-exports
│       │   ├── text_input.rs  # TextInput state machine
│       │   ├── path_input.rs  # PathInput, completions, common-prefix helper
│       │   ├── select.rs      # Select widget and options/actions
│       │   └── progress.rs    # DownloadProgress, log entries, progress callback adapter
│       └── wizard/
│           ├── mod.rs         # run_interactive_flow and step dispatch
│           ├── state.rs       # WizardStep, StepResult, FontChoice, WizardState
│           └── steps.rs       # per-screen wizard step implementations
├── tests/
│   ├── cli.rs                 # clap parsing and option validation tests
│   ├── crawl_chapter.rs       # single-chapter save/skip/overwrite/fast-skip tests
│   ├── crawler.rs             # parser and latest-chapter discovery tests
│   ├── epub.rs                # EPUB metadata, XHTML/OPF/NCX, ZIP integration tests
│   ├── font.rs                # font metadata extraction tests
│   ├── runner.rs              # sequential/parallel runner and progress tests
│   ├── ui.rs                  # widget, progress, and summary tests
│   └── utils.rs               # text, slug, URL, HTTP, filesystem helper tests
├── target/                    # Cargo build output (gitignored)
└── output/                    # default crawler output directory (gitignored)
```

## Runtime flow

- `src/bin/truyenazz-crawl.rs` parses CLI args via `cli`, chooses interactive TUI vs non-interactive mode, builds an `InteractivePlan`, then executes crawl and/or EPUB stages.
- Crawl work goes through `runner` for sequential or parallel execution; each chapter delegates to `crawler::crawl_chapter`.
- `crawler` fetches chapter HTML, extracts titles/paragraphs, applies existing-file policy, and writes `chapter_NNNN.html` under the per-novel output directory.
- `epub` reads saved chapter files, fetches novel metadata/cover, builds XHTML/nav/NCX/OPF assets, embeds font/cover when available, and writes the final EPUB archive.
- `ui` owns the interactive wizard, prompt screens, progress widgets, and loading/download TUI screens; public API remains available through `truyenazz_crawler::ui::*` re-exports.

## CI and release workflows

- `.github/workflows/ci.yml`: runs on pull requests and pushes to `main`; the test job runs first and the supported-platform build matrix uses `needs: test`.
- `.github/workflows/release.yml`: runs on `v*` tags and manual dispatch; builds release artifacts for Linux x86_64, Windows x86_64, macOS Intel, and macOS ARM.

## Auto-memory pointers

`/Users/minhle/.claude/projects/-Users-minhle-dev-truyenazz-crawler/memory/` holds the harness's own auto-memory. Existing entry:

- `feedback_doc_comments.md` — every function in this repo gets a doc comment. This is a user-confirmed override of the default "no comments" rule.
