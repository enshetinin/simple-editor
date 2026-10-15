# Contributing to SimpleEditor

Thanks for your interest! SimpleEditor is a desktop code editor with a
built-in HTTP client, written in Rust for macOS and Windows.

## Ground rules
- Priorities, in order: integrity of the user's work, editing latency,
  stability, simplicity, features.
- Nothing runs network, large file walks or expensive parsing on the UI
  thread. Do not clone or re-parse whole documents per keystroke or frame.
- Never interpret user text (URLs, requests, cURL) as shell commands.
- TLS stays verified; secrets are never written to disk in plain text.
- No telemetry, accounts or background network traffic.
- Keep dependencies few and permissively licensed (MIT, Apache-2.0, BSD,
  ISC, Zlib or compatible); run `python3 scripts/third_party_licenses.py`
  after changing `Cargo.lock`.
- Code and identifiers in English; user-facing strings live in
  `src/app/strings.rs`.

## Setup
- Rust stable (MSRV 1.95, see `Cargo.toml`) via rustup.
- macOS: Xcode Command Line Tools. Windows: Visual Studio Build Tools (MSVC).

## Before opening a pull request
```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
python3 scripts/third_party_licenses.py --check
```
Tests use temporary folders, a local HTTP/HTTPS server and an in-memory
secret store: they never touch your files, network services or Keychain.

Test behaviors, not implementation details. Bugs get a regression test.
Performance-sensitive changes: run
`cargo run --release --example editor_probe -- --100mib` before and after,
and include the numbers.

## Layout
| Path | Contents |
|---|---|
| `src/editor` | Rope document, undo history, Unicode movement, search, encoding, atomic save |
| `src/syntax` | Tree-sitter worker and highlight spans |
| `src/http` | Request model, variables, validation, client, cURL, secrets |
| `src/storage` | Crash recovery, project collections/environments, history, session |
| `src/app` | egui UI; custom editor widget in `editor_view.rs` |
| `tests/` | Integration tests (headless app, local servers) |
| `scripts/` | Icon, licenses and packaging scripts |
| `docs/` | Product specs, status and release checklist |

## License
By contributing you agree that your contributions are licensed under the
MIT license of this repository (`LICENSE`).
