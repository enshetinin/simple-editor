# SimpleEditor

**Escribe código. Prueba APIs. Así de simple.**

SimpleEditor is an open source desktop code editor written in Rust for macOS and
Windows, with a built-in HTTP client. It runs locally: no accounts, no
subscriptions, no telemetry.

> Status: **H0 preview** (feasibility milestone). It opens, edits and saves
> UTF-8 files and sends real HTTP requests with cancellation. It is not yet a
> daily-driver editor: no syntax highlighting, search, line numbers or
> recovery yet, and editing is limited to files up to 2 MiB. See
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

- **Files**: the sidebar lists the project lazily (folders load when
  expanded). Click a file to open it. `Cmd/Ctrl+S` saves, `Cmd/Ctrl+Shift+S`
  saves as, `Cmd/Ctrl+O` opens, `Cmd/Ctrl+N` new file, `Cmd/Ctrl+W` closes the
  tab. Closing a modified tab or the window asks to save, discard or cancel.
- **HTTP**: *New HTTP request* opens a request tab. Choose the method, type an
  `http://` or `https://` URL, an optional body (JSON or text) and press
  **Send** (or `Cmd/Ctrl+Enter`). **Cancel** stops the request. The response
  shows status, time, size, final URL, body and headers. Bodies above 5 MiB
  are truncated in the preview; timeout is 30 s.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo run --release --example editor_probe   # editor view/buffer performance probe
```

The HTTP tests use a local server started by the tests themselves
(`tests/support/mod.rs`); no external services are contacted, except that one
test resolves the reserved `.invalid` domain to check DNS error reporting.

Code layout: `src/editor` (document model, encoding, safe save), `src/http`
(request validation, client, cancellable service), `src/workspace.rs`
(on-demand file tree), `src/cli.rs`, `src/app` (egui UI). Specifications live
in [`docs/`](docs).

## License

MIT — see [LICENSE](LICENSE).
