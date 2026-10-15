# SimpleEditor

**Escribe código. Prueba APIs. Así de simple.**

SimpleEditor is an open source desktop code editor written in Rust for macOS and
Windows, with a built-in HTTP client. It runs locally: no accounts, no
subscriptions, no telemetry.

> Status: **H2 preview**. The editor (H1) and the daily-use HTTP client (H2)
> work: collections, environments, variables, secrets, history and cURL
> import/export. Packaging and installers arrive in H3. See
> [docs/STATUS.md](docs/STATUS.md) for what is verified and what is pending.

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
*New HTTP request* (or **+** in the HTTP section of the sidebar) opens a
request tab:

- **Method + URL**, then **Params**, **Headers** (both with on/off and
  repeated keys), **Body** (None / Text / JSON, with *Format JSON*), **Auth**
  (None / Bearer / Basic), request **Variables** and **Settings** (timeout,
  download limit). **Send** or `Cmd/Ctrl+Enter`; **Cancel** stops it;
  *Send ▾ → Send and save response to file…* streams the body to disk.
- **Variables** `{{name}}` work in URL, params, headers, body and auth.
  Precedence: request variables > active environment > project variables.
  An unknown variable blocks sending and names the field.
- **Environments** and project variables are edited from *✎* in the sidebar.
  Variables marked *Secret* are never written to project files, history or
  exports; they live in memory for the session unless *Remember* stores them
  in the macOS Keychain / Windows Credential Manager. If that store fails, the
  secret stays in memory only (never plain text on disk).
- **Collections** are saved in the project as readable JSON for Git:
  `.simpleeditor/collections/*.json`, `.simpleeditor/environments/*.json`,
  `.simpleeditor/variables.json` (each with `schema_version`). Nothing is
  created until you save. Literal credentials (a token typed directly into
  Auth or an `Authorization` header) are left out when saving; use a secret
  variable instead. Collections can be exported (secrets left out unless you
  choose *Export including secrets*) and imported (never overwriting an
  existing collection).
- **History** keeps the last 100 requests in your user data folder, without
  bodies or credentials.
- **cURL**: *⋯ → Import cURL…* parses a pasted command (never executes it);
  *Copy as cURL* exports for POSIX shells. The exact supported subset is in
  [docs/CURL.md](docs/CURL.md).
- Responses show status, time, size, final URL, headers and the body (pretty
  JSON or raw; invalid JSON, binary, empty and truncated bodies are labeled).
  The preview keeps up to 5 MiB of decompressed data; gzip/deflate/brotli are
  decoded automatically. Redirects are followed up to 10 times and
  `Authorization`/`Cookie` are not sent to another origin. TLS certificates
  are always verified; the timeout is 30 s by default.

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
`src/storage` (crash recovery, project collections/environments, history),
`src/http` (request model, variables, validation, client, cancellable
service, cURL, secrets), `src/workspace.rs` (on-demand file tree), `src/cli.rs`,
`src/app` (egui UI, custom editor widget in `editor_view.rs`).

Set `SE_DATA_DIR` to keep recovery data, history and state in another folder
and `SE_SECRET_BACKEND=memory` to keep secrets in memory only (tests use their
own temporary folders and an in-memory secret store, so they never touch your
real data or Keychain). Specifications live
in [`docs/`](docs).

## License

MIT — see [LICENSE](LICENSE).
