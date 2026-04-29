# Code style and conventions

## Hard rules

- **Doc comment on every fn** (including private helpers). User feedback:
  the codebase must be browsable without reading bodies. One line is fine
  when intent is obvious; expand only when there is a non-obvious WHY
  (constraint, invariant, or workaround). This OVERRIDES the default
  "no comments" guidance from the harness.
- **TDD is the default workflow.** Write the failing test first, watch it
  fail (RED), implement minimally (GREEN), refactor while green. Skill
  reference: `superpowers:test-driven-development` and the project-local
  `.claude/skills/rust-testing/SKILL.md`.
- **No backwards-compat shims** unless requested. Edit existing types/API
  freely; let the compiler push call sites.
- **Doc comment style on public items**: `///` lines that explain purpose,
  invariants, and noteworthy edge cases. Add `# Errors` / `# Examples`
  sections when they earn their keep.

## Module conventions

- Small modules per concern (`utils`, `crawler`, `epub`, …) with
  integration tests under `tests/<module>.rs`.
- Pre-compiled regexes via `once_cell::sync::Lazy<Regex>` at module top.
- Parameter structs (`SequentialParams`, `BuildEpubParams`,
  `ContentOpfParams`, `SummaryParams`) instead of long positional argument
  lists. Use `let X { ... } = params;` destructuring at the function top.
- Errors via `anyhow::Result` for application code. Library-style errors
  use `thiserror` when needed.

## Testing conventions

- Integration tests under `tests/<module>.rs`. Each file imports public
  API only — never `#[path = ...]`.
- HTTP mocked with `mockito::Server::new_async`.
- Filesystem with `tempfile::tempdir()`.
- Test names spell out the behaviour: `crawl_chapter_writes_html_when_file_missing`,
  `parallel_collects_failures_sorted_by_chapter`.
- The TUI run loop is not unit tested (real terminal required); only the
  underlying state machines (`TextInput`, `Select`, `DownloadProgress`).

## Lint floor

Run `cargo clippy --all-targets` until zero warnings before declaring done.
Edition 2024 let-chains are encouraged (`if let Some(x) = ... && cond`).
