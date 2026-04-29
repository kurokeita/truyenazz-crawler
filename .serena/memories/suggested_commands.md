# Suggested commands

All commands run from the repository root (`/Users/minhle/dev/truyenazz-crawler`).

## Build

```fish
cargo build                      # debug build
cargo build --release            # optimised binary at target/release/truyenazz-crawl
```

## Run

```fish
# Non-interactive
cargo run --release -- https://truyenazz.me/your-novel --start 1 --end 50

# Interactive TUI (no positional URL OR -i)
cargo run --release -- -i

# Common flags
#   --workers 4            # parallel; requires --if-exists skip|overwrite
#   --if-exists skip       # ask | skip | overwrite
#   --epub                 # build epub after crawl
#   --epub-only --chapter-dir DIR    # epub from saved chapters only
#   --fast-skip            # skip network when file exists
#   --font-path FILE       # override the bundled Bokerlam.ttf
```

## Test

```fish
cargo test                       # all tests
cargo test --test runner         # one integration file
cargo test some_test_name        # by name pattern
cargo test -- --nocapture        # show println / eprintln
```

## Lint / format

```fish
cargo clippy --all-targets       # MUST be zero warnings
cargo fmt                        # rustfmt
cargo fmt --check                # CI-style format check
```

## Smoke local mock

```fish
# In another shell:
cd /tmp/truyenazz-mock && python3 -m http.server 8765

# Then:
cargo run --release -- http://localhost:8765/foo --start 1 --end 3 \
    --if-exists overwrite --output-root /tmp/crawl-out --delay 0
```

## Darwin (macOS) notes

- Default shell is `fish` for the user. Quote paths with spaces.
- `find` from `.` not `/`.
- `pkill -f "http.server 8765"` to clean up smoke-test servers.
- Use `gh` for GitHub work (no PR pushes without explicit OK from user).
