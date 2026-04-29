# truyenazz-crawler — Project Overview

## Purpose

Rust port of a TypeScript chapter crawler + EPUB builder for the Vietnamese
novel site `truyenazz.me`. Given a novel base URL, it:

1. Discovers the latest available chapter
2. Downloads requested chapter HTML pages (sequentially or in parallel)
3. Saves cleaned, deduplicated chapter HTML to disk
4. Optionally bundles the saved chapters into a styled EPUB with cover and
   embedded font

Two surfaces: a non-interactive CLI (`truyenazz-crawl <url> --start N --end M`)
and an interactive ratatui TUI (`truyenazz-crawl -i`) that walks the user
through every option, then displays a live download progress screen.

## Layout (root of repo)

- `Cargo.toml` — single crate, library + binary
- `src/lib.rs` — declares modules `cli`, `crawler`, `epub`, `font`, `runner`,
  `ui`, `utils`
- `src/bin/truyenazz-crawl.rs` — binary entry point, dispatches between the
  TUI and non-interactive flows and runs the runner + EPUB build
- `Bokerlam.ttf` — bundled Vietnamese font embedded into the EPUB (lookup
  order in `utils::find_font_file`: explicit path → exe dir → exe parent → cwd)
- `tests/` — integration tests organized one file per module
- `typescript/` — original TS implementation kept for reference; will be
  removed once the Rust port is fully validated

## Module map (src/)

| File | Role |
| ------ | ------ |
| `utils.rs` | Pure helpers: `clean_text`, `is_noise`, `slugify`, `build_chapter_url`, plus reqwest-backed `fetch_html` / `download_binary`, async fs helpers |
| `font.rs` | Parses TTF `name` table to derive family + extension; falls back to file stem on malformed buffers |
| `crawler.rs` | HTML parsing (scraper crate), `extract_full_chapter_text`, injected-script content extraction, `discover_last_chapter_number`, `crawl_chapter` with the existing-file policy state machine |
| `epub.rs` | Title/author/cover extraction, XHTML/NCX/OPF rendering, `build_epub` end-to-end (zip crate) |
| `runner.rs` | `crawl_chapters_sequential` / `crawl_chapters_parallel`, emits `ProgressEvent::Started/Completed/Failed` via an optional `ProgressCallback` |
| `cli.rs` | clap derive parser; `parse_from` for tests + `from_raw` for the binary; validators (`validate_shared_options`, `validate_chapter_range`) |
| `ui.rs` | ratatui TUI primitives (`TextInput`, `Select`), `run_interactive_flow`, `DownloadProgress` state machine, `run_download_screen` |

## Key external behaviors

- Existing-file policy: `Ask` / `Skip` / `Overwrite` / `SkipAll` — the
  `Ask` path triggers a prompt callback; `SkipAll` propagates run-wide.
- Parallel mode forbids `--if-exists ask` (CLI-side guard).
- Fast skip: when the destination chapter file exists, skip the network
  fetch entirely.
- Interactive mode shows a styled ratatui download screen with a gauge,
  rolling activity log, and Esc-to-abort.
- Non-interactive mode shows an indicatif progress bar (with plain-line
  fallback when stderr is not a TTY).

## Tests

98 integration tests across `tests/utils.rs`, `tests/font.rs`,
`tests/crawler.rs`, `tests/crawl_chapter.rs`, `tests/epub.rs`,
`tests/runner.rs`, `tests/cli.rs`, `tests/ui.rs`. HTTP is faked with
`mockito`; filesystem with `tempfile`. Build verifies the produced
EPUB zip contains the expected entries.
