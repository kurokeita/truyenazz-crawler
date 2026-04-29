# TruyenAZZ Crawler Overview
- Purpose: TypeScript/Node.js CLI that crawls chapters from TruyenAZZ, saves chapters as HTML, and builds EPUB files.
- Scope: Supports chapter ranges, latest chapter discovery, parallel workers, EPUB-only mode, interactive terminal UI, and custom font selection.
- Runtime: Node.js CommonJS package compiled from TypeScript to `dist/`.
- Primary entrypoint: `dist/cli.js` exposed as the `truyenazz-crawl` bin in `package.json`.
- Assets: `Bokerlam.ttf` is a package asset copied into `dist/` during build so published runs can embed it in EPUB output.
- Main outputs: generated build artifacts in `dist/` and crawled book content in `output/<novel_slug>/`.