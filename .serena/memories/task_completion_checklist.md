# Task completion checklist

Before declaring a task done:

1. **Tests** — `cargo test` returns 0 failures. If you added behaviour,
   you also added (or extended) a test for it under `tests/<module>.rs`.
2. **TDD discipline** — every new function entered the codebase via a
   failing test. If you accidentally wrote production code first, delete
   it and re-derive from a test.
3. **Clippy** — `cargo clippy --all-targets` has 0 warnings (we treat
   warnings as errors here). Edition 2024 let-chains are preferred over
   nested `if let`.
4. **Docs** — every new function has a `///` doc comment explaining its
   purpose. Public types document field invariants.
5. **Release build** — `cargo build --release` succeeds.
6. **Smoke check** — for changes touching the binary, run
   `./target/release/truyenazz-crawl --help` to confirm exit 0 and run
   one happy-path command (or against the local mock server) to verify
   the change is end-to-end correct. UI changes need eye-on-terminal.
7. **No commit / push** unless the user asks. The user owns when to
   commit; never auto-commit. Use `feat:` / `fix:` / `refactor:` /
   `docs:` prefixes when asked. **Never** add `Co-Authored-By: Claude`
   trailers — see `.claude/CLAUDE.md`.
8. **Memory hygiene** — if the task surfaced a new convention or
   correction, save it via `mcp__serena__write_memory` so future
   sessions inherit it.

Skipping any of these means the task is not done.
