# TypeScript Package

This directory contains a TypeScript/Node.js implementation of the TruyenAZZ crawler.

## Scope

It mirrors the current Python CLI behavior:

- crawl chapter ranges
- auto-discover the latest chapter
- save chapters as HTML
- build EPUB files
- support `--epub-only`
- support `--workers`

## Install

```bash
npm install
npm run build
```

## Run

```bash
node dist/cli.js "https://truyenazz.me/your-novel"
```

To launch the interactive TUI instead of passing every option manually:

```bash
node dist/cli.js --interactive
```

The TUI lets you:

- choose between crawling chapters, crawling plus EPUB generation, or EPUB-only mode
- inspect the detected novel title and latest available chapter
- set chapter ranges, worker count, overwrite behavior, and EPUB font options
- use the same terminal UI when deciding what to do with existing chapter files

For local development, you can run the CLI directly from TypeScript without building first:

```bash
pnpm dev -- --interactive
```

`pnpm dev` now runs the TypeScript entrypoint directly, matching the `git-clean-up` package style, so you do not need to build first during local development.

## Published Usage

When published, users can run it with:

```bash
pnpx @kurokeita/truyenazz-crawler "https://truyenazz.me/your-novel"
```

By default, chapters will be written to:

```text
./output/{novel_slug}
```

relative to the caller's current working directory.

To override the default EPUB font, pass:

```bash
pnpx @kurokeita/truyenazz-crawler "https://truyenazz.me/your-novel" --epub --font-path "/path/to/font.ttf"
```

## Notes

- The package embeds the package-local `Bokerlam.ttf`.
- During build, the font is copied into `dist/Bokerlam.ttf` so published `pnpx` runs can still package it into generated EPUB files.
- The runtime font lookup checks:
  - `dist/Bokerlam.ttf`
  - `./Bokerlam.ttf`
  - `../Bokerlam.ttf`
  - `../../Bokerlam.ttf`
- If the font cannot be found, the EPUB still builds with a serif fallback.
