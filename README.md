# SimpleEditor

**Escribe código. Prueba APIs. Así de simple.**

SimpleEditor is an open source desktop code editor written in Rust for macOS and
Windows, with a built-in HTTP client. It runs locally: no accounts, no
subscriptions, no telemetry.

> Status: **H1 preview** (reliable editor milestone). Editing, saving, crash
> recovery, find/replace and syntax highlighting work; the HTTP client is the
> H0 slice (method, URL, body, send/cancel). Collections, variables and
> environments arrive in H2. See [docs/STATUS.md](docs/STATUS.md) for what is
> verified and what is pending.

## Requirements

- Rust stable **1.95 or newer** (developed with 1.99.0) via [rustup](https://rustup.rs).
- macOS: Xcode Command Line Tools (`xcode-select --install`).
- Windows: Visual Studio Build Tools with the "Desktop development with C++"
  workload (MSVC). Not yet verified on a real Windows machine.

## Build and run

```sh
cargo build --release
./target/release/se .                 # open the current folder
./target/release/se src/main.rs       # open files (relative to the current dir)
./target/release/se --help
```

On Windows use `target\release\se.exe .`. The same program is also built as
`simpleeditor` (alternative name). Installing the `se` command on your `PATH`
is not automated yet; you can run it from `target/release/` or copy it
yourself without replacing an existing `se`.

## Using it

### Editor
- The sidebar lists the project lazily (folders load when expanded). Click a
  file to open it. Files are read and saved on background threads.
- Syntax highlighting (Tree-sitter) for Rust, JavaScript, TypeScript/TSX,
  JSON, HTML and CSS. Files over 8 MiB open in large-file mode: highlighting
  is off (shown in the status bar), editing stays fast. Files up to 256 MiB
  can be opened.
- Encoding is UTF-8 with or without BOM; the BOM and LF / CRLF / mixed line
  endings are preserved exactly. Binary and non-UTF-8 files are refused with
  an explanation instead of being converted.
- Unsaved changes are copied to a private recovery folder (never next to
  your files). After a crash, SimpleEditor offers to restore them as unsaved
  changes; the original files are not touched.
- If another program changes an open file, an unmodified tab reloads it; a
  modified tab shows a banner to reload or keep your version, and saving
  never overwrites the other program's version without asking.

| Action | macOS | Windows |
|---|---|---|
| Save / Save as | Cmd+S / Shift+Cmd+S | Ctrl+S / Shift+Ctrl+S |
| Open file / New file | Cmd+O / Cmd+N | Ctrl+O / Ctrl+N |
| Close tab | Cmd+W | Ctrl+W |
| Undo / Redo | Cmd+Z / Shift+Cmd+Z | Ctrl+Z / Ctrl+Y or Shift+Ctrl+Z |
| Find / Replace | Cmd+F / Alt+Cmd+F | Ctrl+F / Ctrl+H |
| Next / previous match | Cmd+G / Shift+Cmd+G, Enter / Shift+Enter in the find bar | F3 / Shift+F3, Ctrl+G |
| Word left/right, delete word | Alt+←/→, Alt+Backspace | Ctrl+←/→, Ctrl+Backspace |
| Line start/end | Cmd+←/→, Home/End | Home/End |
| Document start/end | Cmd+↑/↓ | Ctrl+Home/End |
| Indent / outdent | Tab / Shift+Tab | Tab / Shift+Tab |
| Send HTTP request | Cmd+Enter | Ctrl+Enter |

### HTTP
*New HTTP request* opens a request tab. Choose the method, type an `http://`
or `https://` URL, an optional body (JSON or text) and press **Send** (or
`Cmd/Ctrl+Enter`). **Cancel** stops the request. The response shows status,
time, size, final URL, body and headers. Bodies above 5 MiB are truncated in
the preview; timeout is 30 s.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo run --release --example editor_probe -- --100mib   # engine/view/syntax performance probe
cargo test --test save -- --ignored    # macOS: real "disk full" test on a 1 MiB disk image
```

The HTTP tests use a local server started by the tests themselves
(`tests/support/mod.rs`); no external services are contacted, except that one
test resolves the reserved `.invalid` domain to check DNS error reporting.

Code layout: `src/editor` (rope document, undo history, Unicode movement,
search, encoding, safe save), `src/syntax` (Tree-sitter worker),
`src/storage` (crash recovery), `src/http` (request validation, client,
cancellable service), `src/workspace.rs` (on-demand file tree), `src/cli.rs`,
`src/app` (egui UI, custom editor widget in `editor_view.rs`).

Set `SE_DATA_DIR` to keep recovery data in another folder (tests do this so
they never touch your real data directory). Specifications live
in [`docs/`](docs).

## License

MIT — see [LICENSE](LICENSE).
