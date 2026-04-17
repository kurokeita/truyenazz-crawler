# Task Completion Notes
- Preferred verification after code changes: run `npm run build` to ensure TypeScript compilation succeeds and assets are copied into `dist/`.
- There is currently no dedicated lint, format, or automated test command defined in `package.json`.
- For runtime-sensitive changes, verify the built CLI with `node dist/cli.js --interactive` or a representative crawl command.
- Keep generated artifacts such as `dist/` and crawler output under `output/` out of version control unless the user explicitly asks otherwise.