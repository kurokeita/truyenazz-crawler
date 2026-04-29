# Style and Conventions
- Language: TypeScript with `strict: true` in `tsconfig.json`.
- Module style: CommonJS package output with mostly function-oriented modules rather than classes.
- File organization: CLI orchestration in `src/cli.ts`, crawling/parsing logic in `src/crawler.ts`, EPUB assembly in `src/epub.ts`, plus smaller utility/type modules.
- Naming: camelCase for functions/variables, PascalCase for interfaces/types, uppercase constants for enum-like policy objects.
- Typing: interfaces are used for CLI and result shapes; explicit return types are common on exported/internal helpers.
- Comments/docstrings: minimal; code relies mostly on descriptive names rather than heavy inline documentation.