# cURL import/export: supported subset

SimpleEditor reads cURL commands with its own parser (`src/http/curl.rs`).
**The text is never executed and never passed to a shell.** Anything outside
this subset produces a visible note or an error; nothing is dropped silently.
Tests: `cargo test --lib http::curl`.

## Import

### Command text
- At most 5 MiB.
- The first word must be `curl` or `curl.exe` (a path such as `/usr/bin/curl` is also accepted).
- Only one command: the shell operators `|` `;` `>` `<` `&` outside quotes are an **error**. Paste a single command without pipes, redirections or chaining.

### Quoting (POSIX shell rules)
| Syntax | Result |
|---|---|
| spaces, tabs, newlines | separate words |
| `'text'` | literal, nothing is escaped inside |
| `"text"` | `\"` `\\` `\$` `` \` `` are escaped and `\`+newline is removed; any other backslash is kept |
| `$'text'` (Chrome «Copy as cURL (bash)») | `\n` `\r` `\t` `\0` `\\` `\'` `\"` `\xHH` `\uHHHH` `\UHHHHHHHH` |
| `\` outside quotes | escapes the next character; `\` + newline (LF or CRLF) continues the line |
| `^` + newline (cmd.exe) | accepted as a continuation **with a note**; the rest of cmd.exe syntax is not supported |
| `$VAR`, `$(…)`, `` `…` ``, globs | **not expanded**: kept literally, with a note |
| unclosed quote | error |

### Options
Short options can be combined (`-sSL`), carry their value attached (`-XPOST`, `-H'A: b'`) or use `--option=value`. `--` ends the options.

| Option | Behavior |
|---|---|
| `URL` (positional) or `--url URL` | Request URL, unchanged (query included). Extra URLs → note «only one request is imported». |
| `-X`, `--request M` | Method: GET, POST, PUT, PATCH, DELETE, HEAD or OPTIONS. Any other method is an **error**. |
| `-H`, `--header 'Name: value'` | Header (repeats are kept, in order). `Name;` sends an empty header. `Name:` (curl's «remove header») → note and ignored. |
| `-d`, `--data`, `--data-ascii`, `--data-binary` | Body text. Several are joined with `&`. `@file` → note (files are not read) and the body is left empty. |
| `--data-raw` | Body text; `@` is literal. |
| `--data-urlencode` | `content`, `=content` or `name=content`, URL-encoded and joined with `&`. `@file` and `name@file` → note, ignored. |
| `--json` | Body; adds `Content-Type: application/json` and `Accept: application/json` if missing. `@file` → note, ignored. |
| `-G`, `--get` | The `-d` data goes into the URL query (`?` or `&`) and the method is GET. |
| `-I`, `--head` | HEAD method. |
| `-u`, `--user user:password` | Basic auth (Auth tab). Without `:` → note (curl would ask for the password); password empty. |
| `--oauth2-bearer TOKEN` | Bearer auth (Auth tab). |
| `-A`, `--user-agent` | `User-Agent` header. |
| `-e`, `--referer` | `Referer` header. |
| `-b`, `--cookie 'a=b'` | `Cookie` header. Without `=` (cookie file) → note, ignored. |
| `-m`, `--max-time N` | Request timeout (rounded up to whole seconds). |
| `-L`, `--location` | Note: redirects are always followed (maximum 10). |
| `--compressed` | Note: gzip, deflate and brotli are decompressed automatically. |
| `-k`, `--insecure` | Note: **ignored**, TLS is always verified. |
| `-s -S -v -i -f -# -N -O -J -g -q -0`, `--silent --show-error --verbose --include --fail --fail-with-body --progress-bar --no-buffer --remote-name --remote-header-name --http1.0 --http1.1 --http2 --globoff --disable --tr-encoding --path-as-is` | curl output options: note, ignored. |
| `-F`/`--form`/`--form-string`, `-o`/`--output`, `-T`/`--upload-file`, `-x`/`--proxy`, `-E`/`--cert`, `--key`, `--cacert`, `--capath`, `--resolve`, `-w`/`--write-out`, `--connect-timeout`, `--max-redirs`, `-c`/`--cookie-jar`, `-r`/`--range`, `--retry`, `-z`/`--time-cond`, `--interface`, `-K`/`--config`, `--proto`, `--limit-rate`, `--trace`, `--trace-ascii`, `--unix-socket`, `-Y`/`--speed-limit`, `-y`/`--speed-time` | Not supported: note and ignored, **including their value**. |
| any other `-x`/`--xyz` | Note «unknown or unsupported option». It is assumed to take no value; if it did, check the URL. |

### Rules applied after reading the options (same as curl)
- Method: `-X` if given; otherwise HEAD with `-I`, GET with `-G` or no body, POST if there is a body.
- With data and no `Content-Type`, `Content-Type: application/x-www-form-urlencoded` is added. That header is visible and editable.
- Body type: **JSON** if the `Content-Type` contains `json`, otherwise **Text**.
- An `Authorization: Bearer …` or valid `Authorization: Basic …` header moves to the Auth tab; any other `Authorization` value stays in Headers.
- Request name: `METHOD /path`.

## Export («Copy as cURL»)
- Labeled **POSIX shell (bash, zsh, sh)**. PowerShell and cmd.exe output are **not offered** because they have no tests of their own.
- Quoting: `'…'` with `'` written as `'\''`; simple words without quotes.
- Includes: `-X` (only when it is not the default for the presence of a body), the URL with the active parameters added in order (URL-encoded; text with `{{variables}}` is kept as is), active headers, Auth (`-H 'Authorization: Bearer …'` or `-u 'user:password'`), `Content-Type` from the body type if there is none, `--data-raw` for the body, and `--max-time` if the timeout is not 30 s.
- `{{variables}}` are exported as written, never resolved.
- **Secrets left out by default**: literal values of Auth (token, password) and of sensitive headers/params (Authorization, Cookie, X-Api-Key, or names containing token, secret, password, key…) become `<omitted>`. Values that are only `{{variables}}` are not secrets and are kept.
- Import of the exported text: verified round trip of method, URL, params, headers with quotes, JSON body, Auth with variables and timeout.
