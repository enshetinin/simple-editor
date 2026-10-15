# Changelog

All notable changes to this project. Versions follow semantic versioning
once a first release is published. Nothing has been released yet.

## [Unreleased] — 0.1.0
### Editor
- Rope buffer, transactional undo/redo, grapheme-aware editing, IME,
  clipboard, line numbers, indentation, find/replace.
- Tree-sitter highlighting for Rust, JavaScript, TypeScript/TSX, JSON, HTML
  and CSS; large-file mode above 8 MiB; files up to 256 MiB.
- Exact preservation of BOM and LF/CRLF/mixed line endings; atomic saves;
  external change detection; crash recovery.
- Session restore per project; drag and drop of files and folders.

### HTTP
- Methods, params/headers with repeats, JSON/text bodies, Bearer/Basic auth.
- `{{variables}}` with request > environment > project precedence;
  environments and collections in `.simpleeditor/` (versioned JSON).
- Secrets in memory or, opt-in, in Keychain / Windows Credential Manager.
- History (100 entries, sanitized); cURL import/export (docs/CURL.md).
- 5 MiB decompressed preview, pretty JSON, save response to file.

### Platform
- `se` / `simpleeditor` commands, opt-in `se --install-cli`.
- macOS `.app`/DMG and Windows zip packaging scripts; app icon.
- Accessibility: editor exposes current line and position to screen readers.
