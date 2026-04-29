# Tech stack

- **Edition / toolchain**: Rust 1.95+, edition 2024
- **Async runtime**: `tokio` (multi-thread, features: `fs`, `time`, `sync`,
  `io-util`, `signal`, `macros`, `rt-multi-thread`)
- **HTTP**: `reqwest` 0.13 with `rustls` + `rustls-native-certs` (no native-tls)
- **HTML parsing**: `scraper` 0.26 (with `ego-tree` 0.11 for sibling traversal
  in `discover_last_chapter_number`)
- **CLI**: `clap` 4 with `derive`
- **TUI**: `ratatui` 0.30 + `crossterm` 0.29
- **Progress (non-TUI)**: `indicatif` 0.18
- **Zip / EPUB**: `zip` 8 with `deflate` only
- **Text**: `unicode-normalization` 0.1, `regex` 1, `html-escape` 0.2,
  `mime_guess` 2, `url` 2, `percent-encoding` 2
- **Plumbing**: `anyhow` 1, `thiserror` 2, `once_cell` 1, `futures` 0.3

Test deps: `mockito` 1, `tempfile` 3.

When bumping major versions, watch for `reqwest` feature renames
(`rustls-tls` → `rustls`) and ratatui's evolving widget API.
