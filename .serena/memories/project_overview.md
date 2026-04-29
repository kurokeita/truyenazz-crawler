# truyenazz-crawler — Project Overview

## Purpose

Rust command-line crawler and EPUB builder for TruyenAZZ novels. Given a novel base URL, it can:

1. Discover the latest available chapter from the novel main page.
2. Download requested chapter HTML pages sequentially or in parallel.
3. Extract, clean, and deduplicate chapter paragraphs.
4. Save cleaned chapter HTML files as `chapter_NNNN.html` under a per-novel output directory.
5. Optionally package saved chapters into an EPUB with metadata, cover image, and embedded font support.

The project exposes two user flows:

- Non-interactive CLI: `truyenazz-crawl <url> --start N --end M [--epub]`.
- Interactive ratatui TUI: `truyenazz-crawl -i` or running without a URL launches a wizard, then shows live download/build screens.

## Current repository shape

- `Cargo.toml` — single Rust crate with a library and `truyenazz-crawl` binary.
- `Cargo.lock` — locked dependency graph for reproducible CI/release builds.
- `Bokerlam.ttf` — bundled default font for EPUB embedding.
- `LICENSE` — GPL-3.0 license text.
- `README.md` — user-facing setup, commands, flow diagram, and CI/release notes.
- `.github/workflows/ci.yml` — PR/main CI: run tests first, then build supported platform binaries.
- `.github/workflows/release.yml` — `v*` tag/manual release artifact builds.
- `src/lib.rs` — declares public modules: `cli`, `crawler`, `epub`, `font`, `runner`, `ui`, `utils`.
- `tests/` — integration tests organized by module/domain.

## Module map

| Path | Role |
| --- | --- |
| `src/bin/truyenazz-crawl.rs` | Process entry point; parses CLI, chooses interactive vs non-interactive flow, runs crawl and/or EPUB stages. |
| `src/cli.rs` | clap-derived `RawArgs`, normalized `CliOptions`, parse helpers, shared CLI validators. |
| `src/runner.rs` | Sequential and parallel chapter runners; emits `ProgressEvent` through optional callbacks. |
| `src/utils.rs` | Shared helpers for text cleanup, noise filtering, slugging, URL construction, HTTP fetch/download, async filesystem checks, delay, and font lookup. |
| `src/font.rs` | TTF/OTF metadata extraction for embedded EPUB font family/extension. |
| `src/crawler/` | Chapter-side crawling domain, split into parser/discovery/types/chapter modules with public re-exports from `mod.rs`. |
| `src/crawler/parser.rs` | `ChapterContent`, HTML escaping, chapter HTML parsing, injected backup-content extraction, saved chapter HTML rendering. |
| `src/crawler/discovery.rs` | Latest-chapter discovery from fetched or provided main-page HTML. |
| `src/crawler/types.rs` | Existing-file policies, prompt decisions, crawl statuses, `CrawlChapterParams`, `CrawlResult`. |
| `src/crawler/chapter.rs` | `crawl_chapter`, output path resolution, existing-file action resolution, save/skip/overwrite flow. |
| `src/epub/` | EPUB domain, split into metadata/chapters/package/build modules with public re-exports from `mod.rs`. |
| `src/epub/metadata.rs` | Novel title/status/description/author extraction, cover URL selection, cover extension selection. |
| `src/epub/chapters.rs` | Saved chapter file discovery and `.chapter-title`/`.chapter-content` extraction. |
| `src/epub/package.rs` | XHTML, title page, nav, NCX, OPF, and manifest/spine rendering helpers. |
| `src/epub/build.rs` | End-to-end `build_epub`: fetch metadata/cover, read chapters/font, write ZIP archive. |
| `src/ui/` | Interactive TUI domain with shared terminal helpers and public `ui::*` re-exports. |
| `src/ui/plan.rs` | `CrawlMode`, `InteractivePlan`, `SummaryParams`, and plan summary rendering. |
| `src/ui/widgets/` | Pure widget/progress state machines: text input, path input, select, download progress. |
| `src/ui/screens/` | ratatui screen loops: prompts, loading spinner, download progress. |
| `src/ui/wizard/` | Interactive wizard state, dispatch, and per-step implementations. |

## Key external behaviors

- Existing-file policy: `Ask`, `Skip`, `Overwrite`, and internal run-wide `SkipAll` propagation.
- `--workers > 1` forbids `--if-exists ask`; concurrent workers cannot safely share interactive prompts.
- Fast skip: when `--fast-skip` and a pre-discovered novel title are available, existing destination files can skip the remote chapter fetch entirely.
- Non-interactive mode uses an indicatif progress bar and falls back to plain stderr lines when not attached to a TTY.
- Interactive mode uses ratatui screens for setup, download progress, EPUB build loading, completion notes, and cancellation handling.
- EPUB builds can use the bundled `Bokerlam.ttf` or a custom `--font-path`.
- Public APIs are preserved through module-level re-exports such as `truyenazz_crawler::crawler::*`, `truyenazz_crawler::epub::*`, and `truyenazz_crawler::ui::*`.

## Runtime flow

1. `src/bin/truyenazz-crawl.rs` parses CLI arguments via `cli`.
2. Missing URL or `--interactive` enters `ui::run_interactive_flow`; otherwise the binary builds a plan from CLI options.
3. Non-EPUB-only plans run `runner::crawl_chapters_sequential` or `runner::crawl_chapters_parallel`.
4. Runners call `crawler::crawl_chapter` for each chapter and emit progress events.
5. `crawler` fetches chapter HTML, parses titles/paragraphs, applies existing-file policy, and writes local HTML.
6. If EPUB output is requested, `epub::build_epub` reads saved chapters, fetches metadata/cover, renders package files, and writes the `.epub` archive.

## Tests

Integration tests currently cover:

- `tests/cli.rs` — clap parsing and validation.
- `tests/crawl_chapter.rs` — single-chapter save/skip/overwrite/fast-skip behavior.
- `tests/crawler.rs` — chapter extraction and latest-chapter discovery.
- `tests/epub.rs` — metadata extraction, XHTML/nav/NCX/OPF generation, ZIP integration.
- `tests/font.rs` — font metadata parsing and fallback behavior.
- `tests/runner.rs` — sequential/parallel runner outcomes and progress events.
- `tests/ui.rs` — widget state machines, download progress, prompt summary helpers.
- `tests/utils.rs` — text cleanup, slugging, URL building, HTTP helpers, filesystem helpers.

`cargo test` passes locally. HTTP is mocked with `mockito`; temporary filesystem state uses `tempfile`.

## CI and release

- CI runs on pull requests and pushes to `main`.
- CI test job runs first; platform build matrix uses `needs: test` so builds only start after tests pass.
- Supported build targets: Linux x86_64, Windows x86_64, macOS Intel, macOS ARM.
- Release workflow runs for `v*` tags and manual dispatch, builds archives, and uploads assets to the GitHub Release.
