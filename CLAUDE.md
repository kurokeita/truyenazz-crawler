# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

A Rust crawler + EPUB builder for the Vietnamese novel site `truyenazz.me`.
The crate exposes a library (`truyenazz_crawler`) and one binary
(`truyenazz-crawl`) with two surfaces:

- **Non-interactive CLI** — clap-driven, takes a positional novel URL and
  flags. Drives a sequential or parallel runner, emits an `indicatif`
  progress bar with plain-line fallback when stdout is not a TTY.
- **Interactive TUI** — ratatui flow that walks the user through every
  option, then displays a live download progress screen with a gauge,
  rolling activity log, and Esc-to-abort. Triggered by `-i` or by passing
  no positional URL.

## Common commands

```fish
# build
cargo build --release           # → target/release/truyenazz-crawl

# tests (98 currently)
cargo test                      # everything
cargo test --test runner        # one integration file
cargo test build_chapter_url    # one test by name pattern
cargo test -- --nocapture       # surface println/eprintln

# lint and format
cargo clippy --all-targets      # MUST be 0 warnings (CI floor)
cargo fmt                       # rustfmt
cargo fmt --check               # check-only

# run
cargo run --release -- <url> --start 1 --end 50           # CLI
cargo run --release -- -i                                  # TUI
cargo run --release -- <url> --epub-only --chapter-dir D   # EPUB-only
```

A throwaway local mock is the easiest way to smoke-test end-to-end:

```fish
# fixture site under /tmp/truyenazz-mock with /foo/index.html and /foo/chuong-N/index.html
cd /tmp/truyenazz-mock && python3 -m http.server 8765 &
cargo run --release -- http://localhost:8765/foo --start 1 --end 3 \
    --if-exists overwrite --output-root /tmp/crawl-out --delay 0
```

## Architecture (the "big picture")

The library is a small layered crate where each module has a single role:

``` shell
cli       ─────────────┐
                       ├─→ runner ─→ crawler ─→ utils  (text, http, fs)
ui (ratatui)  ─────────┘                      ↘ epub  ─→ font (TTF parsing)
```

- **`utils`** — pure helpers (`clean_text`, `is_noise`, `slugify`,
  `build_chapter_url`) plus reqwest-backed `fetch_html`/`download_binary`
  and async fs primitives. Everything I/O-related funnels through here so
  tests can swap in `mockito` and `tempfile`.

- **`crawler`** — parses one chapter HTML with `scraper`, runs noise
  filtering and consecutive-line dedup, extracts injected JS-hidden
  paragraphs, builds the saved-on-disk chapter HTML, and exposes
  `crawl_chapter` plus the **existing-file policy state machine**
  (`Ask` / `Skip` / `Overwrite` / `SkipAll`). The `Ask` path takes a
  prompt callback so the TUI and the stdin readline can plug in.
  `discover_last_chapter_number` walks the "Chương Mới Nhất" section to
  find the latest available chapter.

- **`runner`** — `crawl_chapters_sequential` and
  `crawl_chapters_parallel` consume chapter ranges and call
  `crawl_chapter` repeatedly. **Sequential propagates `SkipAll` run-wide**
  so once a user picks "skip all" the rest of the run never prompts again.
  Both runners emit `ProgressEvent::Started/Completed/Failed` through an
  optional `Arc<dyn Fn(ProgressEvent) + Send + Sync>` callback. The CLI
  guards against `--workers > 1 && --if-exists ask`.

- **`epub`** — pulls metadata (title, author, cover) from the novel's
  main page, loads each saved chapter HTML, renders XHTML/NCX/OPF, and
  zips an EPUB 3 (mimetype is the first STORE-compressed entry per spec).
  Cover and bundled `Bokerlam.ttf` are embedded when present; cover
  extension is picked first from the response Content-Type, then the URL
  path, then `.jpg`.

- **`font`** — best-effort TTF `name`-table parser. On malformed input
  it falls back to the file stem so EPUB build never crashes.

- **`cli`** — clap derive `RawArgs` and a normalized `CliOptions` (with
  `from_raw` for the binary, `parse_from` for tests). Holds the
  validators (`validate_shared_options`, `validate_chapter_range`).

- **`ui`** — three layers stacked together:
  1. `TextInput` and `Select` widgets with pure state machines (unit
     tested without a real terminal).
  2. `run_text_prompt` / `run_select` / `show_note` — synchronous ratatui
     screens used during the interactive plan flow. Each opens its own
     `TerminalGuard` (raw mode + alt screen), so the TUI is always torn
     down between prompts.
  3. `run_interactive_flow` walks the screens and returns an
     `InteractivePlan`. `DownloadProgress` + `run_download_screen` is the
     download stage: the runner is `tokio::spawn`ed, a shared
     `Arc<Mutex<DownloadProgress>>` is updated by the progress callback,
     and the render loop polls keys with an 80ms timeout while watching
     `runner_task.is_finished()`.

The binary (`src/bin/truyenazz-crawl.rs`) only orchestrates: parse CLI →
either build a non-interactive plan (with `discover_last_chapter_number`

- end-clamping) or `run_interactive_flow` → execute the plan with the TUI
download screen if interactive, else with the indicatif bar → optionally
build the EPUB → exit `0` (success), `2` (partial failures), or `3`
(EPUB build failed).

## Style and discipline

- **TDD is the workflow.** Every new function enters the codebase via a
  test in `tests/<module>.rs` that fails first, then a minimal
  implementation. The `superpowers:test-driven-development` skill is the
  reference; the project-local `.claude/skills/rust-testing/SKILL.md`
  documents Rust-specific patterns.
- **Doc-comment every fn.** This is a user-confirmed override of the
  default "no comments" guidance. One line is fine when intent is
  obvious; expand only when there is a non-obvious WHY.
- **Edition 2024 let-chains.** Prefer
  `if let Some(x) = ... && cond { ... }` over nested `if let { if cond }`.
- **Parameter structs** for >5-arg functions
  (`SequentialParams`, `BuildEpubParams`, `ContentOpfParams`,
  `SummaryParams`). Destructure at the top of the function.
- **Pre-compile regexes** with `once_cell::sync::Lazy<Regex>` at the
  module scope.
- **Errors:** `anyhow::Result` for application code; `thiserror` if
  library-style typed errors are needed.
- **No backwards-compat shims.** Edit the API freely; let the compiler
  push call sites.
- **Never auto-commit.** The user owns when to commit. Commit prefixes:
  `feat:` / `fix:` / `refactor:` / `docs:`. **Never** add
  `Co-Authored-By: Claude` trailers — see the user's global CLAUDE.md.

## Test conventions

- Integration tests live under `tests/<module>.rs` and exercise only the
  public API.
- HTTP is mocked with `mockito::Server::new_async`.
- Filesystem with `tempfile::tempdir()`.
- Test names spell out the behaviour:
  `crawl_chapter_writes_html_when_file_missing`,
  `parallel_collects_failures_sorted_by_chapter`.
- The TUI run loop is **not** unit-tested (real terminal required); only
  the underlying state machines (`TextInput`, `Select`,
  `DownloadProgress`).

## Definition of done

- `cargo test` green (98+ tests).
- `cargo clippy --all-targets` 0 warnings.
- `cargo build --release` succeeds.
- Every new fn has a `///` doc comment.
- For UI-touching changes, eyes-on-terminal verification —
  `cargo test` proves logic but not what the screen looks like.

## Useful project context

- **Bundled font:** `Bokerlam.ttf` at the repo root is embedded into the
  EPUB. `utils::find_font_file` looks (in order): explicit `--font-path`,
  exe dir, exe parent, cwd. Missing font is non-fatal — EPUB falls back
  to a generic serif family.
- **Chapter URL convention:** `<base>/chuong-<N>/`. Output files are
  named `chapter_NNNN.html` (zero-padded).
- **Default output:** `./output/<novel_slug>/`. Override with
  `--output-root`.
- **Fast skip:** when `--fast-skip` is set and the destination chapter
  file already exists, the network fetch is skipped entirely.
- **Serena** is configured with `language: rust` in `.serena/project.yml`
  — restart the Serena session after a language-server upgrade.
- **Auto-memory pointer:** the harness's auto-memory directory at
  `~/.claude/projects/-Users-minhle-dev-truyenazz-crawler/memory/` holds
  `feedback_doc_comments.md` (the doc-comment-every-fn rule). Project
  conventions live as Serena memories under `.serena/memories/`.
