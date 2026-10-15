//! cURL command import/export.
//!
//! The command text is tokenized by an explicit parser that understands
//! POSIX shell quoting; **nothing is ever executed** and no shell expansion
//! (`$VAR`, `$(…)`, backticks, globs) happens. The supported subset is
//! documented in `docs/CURL.md`; anything else produces a warning, never a
//! silent drop.

use super::model::{Auth, Body, BodyMode, KeyValue, RequestDefinition};
use super::request::Method;
use super::sanitize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlImport {
    pub request: RequestDefinition,
    pub warnings: Vec<String>,
}

/// Largest command accepted (a cURL with a big inline body).
pub const MAX_COMMAND_BYTES: usize = 5 * 1024 * 1024;

// ---- tokenizer ---------------------------------------------------------------

/// Splits a command line into words using POSIX shell quoting rules.
pub fn tokenize(input: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let mut words = Vec::new();
    let mut warnings = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = input.chars().peekable();
    let mut warned_expansion = false;
    let mut warn_expansion = |warnings: &mut Vec<String>| {
        if !warned_expansion {
            warned_expansion = true;
            warnings.push(
                "Shell expansions ($VAR, $(…), `…`) are not performed; the text was kept literally."
                    .to_owned(),
            );
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('\n') => {}
                Some('\r') if chars.peek() == Some(&'\n') => {
                    chars.next();
                }
                Some(n) => {
                    cur.push(n);
                    in_word = true;
                }
                None => {}
            },
            '^' if matches!(chars.peek(), Some('\n' | '\r')) => {
                // cmd.exe line continuation.
                while matches!(chars.peek(), Some('\n' | '\r')) {
                    chars.next();
                }
                if !warnings.iter().any(|w: &String| w.contains("cmd.exe")) {
                    warnings.push(
                        "Windows cmd.exe syntax (^) is only partly supported; check the imported request."
                            .to_owned(),
                    );
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(ch) => cur.push(ch),
                        None => return Err("unterminated single quote (')".to_owned()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(ch @ ('"' | '\\' | '$' | '`')) => cur.push(ch),
                            Some('\n') => {}
                            Some(ch) => {
                                cur.push('\\');
                                cur.push(ch);
                            }
                            None => return Err("unterminated double quote (\")".to_owned()),
                        },
                        Some(ch @ ('$' | '`')) => {
                            warn_expansion(&mut warnings);
                            cur.push(ch);
                        }
                        Some(ch) => cur.push(ch),
                        None => return Err("unterminated double quote (\")".to_owned()),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                in_word = true;
                ansi_c_string(&mut chars, &mut cur)?;
            }
            '$' | '`' => {
                warn_expansion(&mut warnings);
                cur.push(c);
                in_word = true;
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '|' | ';' | '>' | '<' | '&' => {
                return Err(format!(
                    "shell operator '{c}' found: paste a single curl command without pipes, redirections or chaining"
                ));
            }
            c => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    Ok((words, warnings))
}

fn ansi_c_string(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    out: &mut String,
) -> Result<(), String> {
    let hex = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>, max: usize| {
        let mut v = 0u32;
        let mut n = 0;
        while n < max {
            match chars.peek().and_then(|c| c.to_digit(16)) {
                Some(d) => {
                    v = v * 16 + d;
                    chars.next();
                    n += 1;
                }
                None => break,
            }
        }
        (v, n)
    };
    loop {
        match chars.next() {
            Some('\'') => return Ok(()),
            Some('\\') => match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('0') => out.push('\0'),
                Some('\\') => out.push('\\'),
                Some('\'') => out.push('\''),
                Some('"') => out.push('"'),
                Some('x') => {
                    let (v, n) = hex(chars, 2);
                    if n == 0 {
                        out.push_str("\\x");
                    } else {
                        // Bytes above 0x7F are not valid UTF-8 on their own.
                        out.push(char::from_u32(v).unwrap_or('\u{FFFD}'));
                    }
                }
                Some(u @ ('u' | 'U')) => {
                    let (v, n) = hex(chars, if u == 'u' { 4 } else { 8 });
                    match (n, char::from_u32(v)) {
                        (1.., Some(c)) => out.push(c),
                        _ => {
                            out.push('\\');
                            out.push(u);
                        }
                    }
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => return Err("unterminated $'…' string".to_owned()),
            },
            Some(c) => out.push(c),
            None => return Err("unterminated $'…' string".to_owned()),
        }
    }
}

// ---- parser ------------------------------------------------------------------

/// Options that take a value but are not supported (value skipped, warned).
const UNSUPPORTED_WITH_VALUE: &[&str] = &[
    "-F",
    "--form",
    "--form-string",
    "-o",
    "--output",
    "-T",
    "--upload-file",
    "-x",
    "--proxy",
    "-E",
    "--cert",
    "--key",
    "--cacert",
    "--capath",
    "--resolve",
    "-w",
    "--write-out",
    "--connect-timeout",
    "--max-redirs",
    "-c",
    "--cookie-jar",
    "-r",
    "--range",
    "--retry",
    "-z",
    "--time-cond",
    "--interface",
    "-K",
    "--config",
    "--proto",
    "--limit-rate",
    "--trace",
    "--trace-ascii",
    "--unix-socket",
    "-Y",
    "--speed-limit",
    "-y",
    "--speed-time",
];

/// Flags that only change curl's own output or behavior; ignored with a note.
const IGNORED_FLAGS: &[&str] = &[
    "-s",
    "--silent",
    "-S",
    "--show-error",
    "-v",
    "--verbose",
    "-i",
    "--include",
    "-f",
    "--fail",
    "--fail-with-body",
    "-#",
    "--progress-bar",
    "-N",
    "--no-buffer",
    "-O",
    "--remote-name",
    "-J",
    "--remote-header-name",
    "--http1.1",
    "--http2",
    "--http1.0",
    "-0",
    "--globoff",
    "-g",
    "-q",
    "--disable",
    "--tr-encoding",
    "--path-as-is",
];

/// Short options that take a value, for splitting `-XPOST` / `-sSL` bundles.
const SHORT_WITH_VALUE: &str = "XHdAeburmFoTxEcwzrKYy";

struct Parsed {
    method: Option<Method>,
    url: Option<String>,
    headers: Vec<KeyValue>,
    data: Vec<String>,
    json: Vec<String>,
    get: bool,
    head: bool,
    auth: Auth,
    timeout_secs: Option<u32>,
    warnings: Vec<String>,
}

pub fn parse(input: &str) -> Result<CurlImport, String> {
    if input.len() > MAX_COMMAND_BYTES {
        return Err(format!(
            "the command is larger than {} MiB",
            MAX_COMMAND_BYTES / (1024 * 1024)
        ));
    }
    let (words, mut warnings) = tokenize(input.trim())?;
    let mut iter = words.into_iter().peekable();
    match iter.next() {
        Some(first) if is_curl(&first) => {}
        Some(first) => return Err(format!("the command must start with curl, not '{first}'")),
        None => return Err("the command is empty".to_owned()),
    }

    // Expand bundled short options: -sSL, -XPOST, -H'…'.
    let mut args: Vec<String> = Vec::new();
    let mut options_done = false;
    for w in iter {
        if !options_done && w == "--" {
            options_done = true;
            continue;
        }
        if options_done || !w.starts_with('-') || w.starts_with("--") || w.len() <= 2 {
            args.push(w);
            continue;
        }
        let body: Vec<char> = w[1..].chars().collect();
        let mut i = 0;
        while i < body.len() {
            let c = body[i];
            args.push(format!("-{c}"));
            if SHORT_WITH_VALUE.contains(c) {
                let rest: String = body[i + 1..].iter().collect();
                if !rest.is_empty() {
                    args.push(rest);
                }
                break;
            }
            i += 1;
        }
    }

    let mut p = Parsed {
        method: None,
        url: None,
        headers: Vec::new(),
        data: Vec::new(),
        json: Vec::new(),
        get: false,
        head: false,
        auth: Auth::None,
        timeout_secs: None,
        warnings: Vec::new(),
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_owned(), Some(v.to_owned())),
            _ => (arg.clone(), None),
        };
        let mut value = |what: &str| -> Result<String, String> {
            match inline.clone().or_else(|| args.next()) {
                Some(v) => Ok(v),
                None => Err(format!("{what} needs a value")),
            }
        };
        match name.as_str() {
            "-X" | "--request" => {
                let m = value(&name)?;
                match Method::parse(&m) {
                    Some(m) => p.method = Some(m),
                    None => return Err(format!("method {m} is not supported")),
                }
            }
            "-H" | "--header" => {
                let h = value(&name)?;
                header(&h, &mut p);
            }
            "-d" | "--data" | "--data-ascii" | "--data-binary" => {
                let d = value(&name)?;
                if let Some(file) = d.strip_prefix('@') {
                    p.warnings.push(format!(
                        "{name} @{file}: reading files is not supported; the body was left empty."
                    ));
                } else {
                    p.data.push(d);
                }
            }
            "--data-raw" => p.data.push(value(&name)?),
            "--data-urlencode" => {
                let d = value(&name)?;
                match urlencode_arg(&d) {
                    Some(enc) => p.data.push(enc),
                    None => p.warnings.push(format!(
                        "--data-urlencode {d}: reading files is not supported; ignored."
                    )),
                }
            }
            "--json" => {
                let d = value(&name)?;
                if d.starts_with('@') {
                    p.warnings.push(format!(
                        "--json {d}: reading files is not supported; ignored."
                    ));
                } else {
                    p.json.push(d);
                }
            }
            "-G" | "--get" => p.get = true,
            "-I" | "--head" => p.head = true,
            "-u" | "--user" => {
                let u = value(&name)?;
                let (user, pass) = match u.split_once(':') {
                    Some((a, b)) => (a.to_owned(), b.to_owned()),
                    None => {
                        p.warnings.push(
                            "-u without a password: curl would prompt for it; enter it in Auth."
                                .to_owned(),
                        );
                        (u, String::new())
                    }
                };
                p.auth = Auth::Basic {
                    username: user,
                    password: pass,
                };
            }
            "--oauth2-bearer" => {
                p.auth = Auth::Bearer {
                    token: value(&name)?,
                }
            }
            "-A" | "--user-agent" => p.headers.push(KeyValue::new("User-Agent", value(&name)?)),
            "-e" | "--referer" => p.headers.push(KeyValue::new("Referer", value(&name)?)),
            "-b" | "--cookie" => {
                let c = value(&name)?;
                if c.contains('=') {
                    p.headers.push(KeyValue::new("Cookie", c));
                } else {
                    p.warnings.push(format!(
                        "{name} {c}: cookie files are not supported; ignored."
                    ));
                }
            }
            "--url" => set_url(value(&name)?, &mut p),
            "-m" | "--max-time" => {
                let v = value(&name)?;
                match v.parse::<f64>() {
                    Ok(secs) if secs > 0.0 => p.timeout_secs = Some(secs.ceil() as u32),
                    _ => p
                        .warnings
                        .push(format!("{name} {v}: invalid time; ignored.")),
                }
            }
            "-L" | "--location" => p.warnings.push(
                "-L: redirects are always followed (up to 10); the option is not needed."
                    .to_owned(),
            ),
            "--compressed" => p.warnings.push(
                "--compressed: gzip, deflate and brotli responses are decompressed automatically."
                    .to_owned(),
            ),
            "-k" | "--insecure" => p.warnings.push(
                "-k/--insecure ignored: SimpleEditor always verifies TLS certificates.".to_owned(),
            ),
            n if UNSUPPORTED_WITH_VALUE.contains(&n) => {
                let v = value(n).unwrap_or_default();
                p.warnings.push(format!("{n} {v}: not supported; ignored."));
            }
            n if IGNORED_FLAGS.contains(&n) => {
                p.warnings.push(format!("{n}: output option ignored."));
            }
            n if n.starts_with('-') && n.len() > 1 => {
                p.warnings.push(format!(
                    "{n}: unknown or unsupported option; ignored (if it takes a value, check the URL)."
                ));
            }
            _ => set_url(arg, &mut p),
        }
    }

    let Some(url) = p.url.clone() else {
        return Err("no URL found in the command".to_owned());
    };
    let mut url = url;
    let mut headers = p.headers;
    let has_header =
        |headers: &[KeyValue], n: &str| headers.iter().any(|h| h.key.eq_ignore_ascii_case(n));

    // Body and method, following curl's rules.
    let mut body = Body::default();
    let mut method = p.method;
    if !p.json.is_empty() {
        body.text = p.json.concat();
        if !has_header(&headers, "content-type") {
            headers.push(KeyValue::new("Content-Type", "application/json"));
        }
        if !has_header(&headers, "accept") {
            headers.push(KeyValue::new("Accept", "application/json"));
        }
    }
    if !p.data.is_empty() {
        let joined = p.data.join("&");
        if p.get {
            let sep = if url.contains('?') { '&' } else { '?' };
            url = format!("{url}{sep}{joined}");
        } else {
            if !body.text.is_empty() {
                body.text.push('&');
            }
            body.text.push_str(&joined);
            if !has_header(&headers, "content-type") {
                headers.push(KeyValue::new(
                    "Content-Type",
                    "application/x-www-form-urlencoded",
                ));
            }
        }
    }
    if !body.text.is_empty() {
        let json_ct = headers.iter().any(|h| {
            h.key.eq_ignore_ascii_case("content-type")
                && h.value.to_ascii_lowercase().contains("json")
        });
        body.mode = if json_ct {
            BodyMode::Json
        } else {
            BodyMode::Text
        };
    }
    if method.is_none() {
        method = Some(if p.head {
            Method::Head
        } else if p.get || body.text.is_empty() {
            Method::Get
        } else {
            Method::Post
        });
    }
    let method = method.unwrap_or(Method::Get);

    // An Authorization header becomes the Auth tab when it is plain Bearer/Basic.
    let mut auth = p.auth;
    if matches!(auth, Auth::None)
        && let Some(pos) = headers
            .iter()
            .position(|h| h.key.eq_ignore_ascii_case("authorization"))
    {
        let v = headers[pos].value.trim().to_owned();
        if let Some(t) = v.strip_prefix("Bearer ") {
            auth = Auth::Bearer {
                token: t.trim().to_owned(),
            };
            headers.remove(pos);
        } else if let Some(b) = v.strip_prefix("Basic ")
            && let Some(decoded) = super::resolve::base64_decode(b.trim())
            && let Ok(text) = String::from_utf8(decoded)
            && let Some((u, pw)) = text.split_once(':')
        {
            auth = Auth::Basic {
                username: u.to_owned(),
                password: pw.to_owned(),
            };
            headers.remove(pos);
        }
    }

    warnings.extend(p.warnings);
    let name = request_name(method, &url);
    Ok(CurlImport {
        request: RequestDefinition {
            id: String::new(),
            name,
            method,
            url,
            params: Vec::new(),
            headers,
            body,
            auth,
            variables: Vec::new(),
            timeout_secs: p.timeout_secs.unwrap_or(super::model::DEFAULT_TIMEOUT_SECS),
        },
        warnings,
    })
}

fn is_curl(word: &str) -> bool {
    let base = word.rsplit(['/', '\\']).next().unwrap_or(word);
    base.eq_ignore_ascii_case("curl") || base.eq_ignore_ascii_case("curl.exe")
}

fn set_url(u: String, p: &mut Parsed) {
    if p.url.is_none() {
        p.url = Some(u);
    } else {
        p.warnings.push(format!(
            "Extra URL {u} ignored: only one request is imported."
        ));
    }
}

fn header(h: &str, p: &mut Parsed) {
    if let Some(name) = h.strip_suffix(';')
        && !name.contains(':')
    {
        // curl: "Name;" sends an empty header.
        p.headers.push(KeyValue::new(name.trim(), ""));
        return;
    }
    match h.split_once(':') {
        Some((k, v)) if v.trim().is_empty() => p.warnings.push(format!(
            "-H '{k}:' removes a default curl header; SimpleEditor does not send it either way, ignored."
        )),
        Some((k, v)) => p.headers.push(KeyValue::new(k.trim(), v.trim_start())),
        None => p
            .warnings
            .push(format!("-H '{h}': not a 'Name: value' header; ignored.")),
    }
}

/// `--data-urlencode` forms: `content`, `=content`, `name=content`;
/// `@file` and `name@file` read files and are not supported.
fn urlencode_arg(arg: &str) -> Option<String> {
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    match arg.split_once('=') {
        Some(("", value)) => Some(enc(value)),
        Some((name, value)) => Some(format!("{name}={}", enc(value))),
        None if arg.contains('@') => None,
        None => Some(enc(arg)),
    }
}

fn request_name(method: Method, url: &str) -> String {
    let path = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    let path = path.split_once('/').map_or("/", |(_, p)| p);
    format!("{} /{}", method.as_str(), path.trim_start_matches('/'))
}

// ---- export --------------------------------------------------------------------

/// Quotes a word for POSIX shells (bash, zsh, sh).
pub fn posix_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c))
    {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Exports a request definition as a cURL command for POSIX shells.
/// `{{variables}}` are kept as written. Unless `include_secrets`, literal
/// credentials (auth fields and sensitive headers) are replaced with
/// `<omitted>`.
pub fn export_posix(def: &RequestDefinition, include_secrets: bool) -> String {
    let def = if include_secrets {
        def.clone()
    } else {
        sanitize::strip_secrets(def).0
    };
    let omitted = |v: &str| {
        if v.is_empty() {
            "<omitted>".to_owned()
        } else {
            v.to_owned()
        }
    };
    let mut url = def.url.clone();
    let params: Vec<_> = def
        .params
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
        .collect();
    if !params.is_empty() {
        let enc = |s: &str| {
            // Keep {{templates}} readable.
            if s.contains("{{") {
                s.to_owned()
            } else {
                url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
            }
        };
        let q: Vec<String> = params
            .iter()
            .map(|p| format!("{}={}", enc(&p.key), enc(&p.value)))
            .collect();
        let sep = if url.contains('?') { '&' } else { '?' };
        url = format!("{url}{sep}{}", q.join("&"));
    }
    let mut parts = vec!["curl".to_owned()];
    let has_body = def.body.mode != BodyMode::None && !def.body.text.is_empty();
    let default_method = if has_body { Method::Post } else { Method::Get };
    if def.method != default_method {
        parts.push(format!("-X {}", def.method.as_str()));
    }
    parts.push(posix_quote(&url));
    for h in def
        .headers
        .iter()
        .filter(|h| h.enabled && !h.key.is_empty())
    {
        let value = if sanitize::is_sensitive_header(&h.key) {
            omitted(&h.value)
        } else {
            h.value.clone()
        };
        parts.push(format!(
            "-H {}",
            posix_quote(&format!("{}: {value}", h.key))
        ));
    }
    match &def.auth {
        Auth::None => {}
        Auth::Bearer { token } => parts.push(format!(
            "-H {}",
            posix_quote(&format!(
                "Authorization: Bearer {}",
                omitted(token.as_str())
            ))
        )),
        Auth::Basic { username, password } => parts.push(format!(
            "-u {}",
            posix_quote(&format!("{username}:{}", omitted(password.as_str())))
        )),
    }
    if has_body {
        let has_ct = def
            .headers
            .iter()
            .any(|h| h.enabled && h.key.eq_ignore_ascii_case("content-type"));
        if !has_ct && let Some(ct) = def.body.mode.content_type() {
            parts.push(format!(
                "-H {}",
                posix_quote(&format!("Content-Type: {ct}"))
            ));
        }
        parts.push(format!("--data-raw {}", posix_quote(&def.body.text)));
    }
    if def.timeout_secs != super::model::DEFAULT_TIMEOUT_SECS {
        parts.push(format!("--max-time {}", def.timeout_secs));
    }
    parts.join(" \\\n  ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn import(cmd: &str) -> CurlImport {
        parse(cmd).unwrap_or_else(|e| panic!("{cmd}: {e}"))
    }

    #[test]
    fn quoting_rules() {
        let (w, _) = tokenize(r#"curl 'a b' "c \"d\" \$e" f\ g $'h\ni\'jé' "x\y""#).unwrap();
        assert_eq!(
            w,
            vec!["curl", "a b", "c \"d\" $e", "f g", "h\ni'jé", "x\\y"]
        );
        let (w, _) = tokenize("curl \\\n  -H 'A: b' \\\r\n  https://x").unwrap();
        assert_eq!(w, vec!["curl", "-H", "A: b", "https://x"]);
        assert!(tokenize("curl 'open").is_err());
        assert!(tokenize("curl \"open").is_err());
        assert!(
            tokenize("curl https://x | sh")
                .unwrap_err()
                .contains("operator")
        );
        assert!(tokenize("curl https://x; rm -rf ~").is_err());
    }

    #[test]
    fn shell_expansion_is_never_performed() {
        let r = import(r#"curl "https://h/$(whoami)?u=$USER" -H "X: `id`""#);
        assert_eq!(r.request.url, "https://h/$(whoami)?u=$USER");
        assert_eq!(r.request.headers[0].value, "`id`");
        assert!(r.warnings.iter().any(|w| w.contains("not performed")));
    }

    #[test]
    fn chrome_style_post_json() {
        let r = import(
            r#"curl 'https://api.example.com/v1/items?x=1' \
  -H 'accept: application/json' \
  -H 'authorization: Bearer abc.def' \
  -H 'content-type: application/json' \
  --data-raw '{"name":"café","n":1}' \
  --compressed"#,
        );
        let q = &r.request;
        assert_eq!(q.method, Method::Post);
        assert_eq!(q.url, "https://api.example.com/v1/items?x=1");
        assert_eq!(q.body.mode, BodyMode::Json);
        assert_eq!(q.body.text, r#"{"name":"café","n":1}"#);
        assert_eq!(
            q.auth,
            Auth::Bearer {
                token: "abc.def".into()
            }
        );
        assert_eq!(q.headers.len(), 2);
        assert!(r.warnings.iter().any(|w| w.contains("--compressed")));
    }

    #[test]
    fn methods_data_get_head_user_and_bundles() {
        let r = import("curl -sSL -XPUT https://h/x -d a=1 -d b=2");
        assert_eq!(r.request.method, Method::Put);
        assert_eq!(r.request.body.text, "a=1&b=2");
        assert_eq!(r.request.body.mode, BodyMode::Text);
        assert!(
            r.request
                .headers
                .iter()
                .any(|h| h.value == "application/x-www-form-urlencoded")
        );
        assert_eq!(r.warnings.len(), 3, "{:?}", r.warnings); // -s -S -L

        let r = import("curl -G https://h/s?q=1 --data-urlencode 'term=a b&c' -d page=2");
        assert_eq!(r.request.method, Method::Get);
        assert_eq!(r.request.url, "https://h/s?q=1&term=a+b%26c&page=2");
        assert_eq!(r.request.body.mode, BodyMode::None);

        assert_eq!(import("curl -I https://h/").request.method, Method::Head);
        let r = import("curl -u ana:p%40ss https://h/");
        assert_eq!(
            r.request.auth,
            Auth::Basic {
                username: "ana".into(),
                password: "p%40ss".into()
            }
        );
        let r = import("curl --json '{\"a\":1}' https://h/");
        assert_eq!(r.request.method, Method::Post);
        assert_eq!(r.request.body.mode, BodyMode::Json);
        assert_eq!(r.request.headers.len(), 2);
    }

    #[test]
    fn unsupported_options_warn_and_are_never_silently_dropped() {
        let r = import("curl -k -F file=@x.png -o out.bin --frobnicate https://h/ https://other/");
        let w = r.warnings.join("\n");
        assert!(w.contains("-k/--insecure"));
        assert!(w.contains("-F file=@x.png"));
        assert!(w.contains("-o out.bin"));
        assert!(w.contains("--frobnicate"));
        assert!(w.contains("Extra URL https://other/"));
        assert_eq!(r.request.url, "https://h/");
        let r = import("curl -d @body.json https://h/");
        assert!(r.warnings[0].contains("reading files"));
        assert!(parse("wget https://h/").is_err());
        assert!(parse("curl -X TRACE https://h/").is_err());
        assert!(parse("curl -H").is_err());
    }

    #[test]
    fn export_then_import_round_trips() {
        let def = RequestDefinition {
            id: String::new(),
            name: "x".into(),
            method: Method::Patch,
            url: "{{base}}/items/1".into(),
            params: vec![KeyValue::new("tag", "a b"), KeyValue::new("tag", "ñ")],
            headers: vec![KeyValue::new("X-Quote", "it's \"q\"")],
            body: Body {
                mode: BodyMode::Json,
                text: "{\"msg\": \"it's\"}".into(),
            },
            auth: Auth::Bearer {
                token: "{{token}}".into(),
            },
            variables: Vec::new(),
            timeout_secs: 12,
        };
        let cmd = export_posix(&def, false);
        let back = import(&cmd).request;
        assert_eq!(back.method, Method::Patch);
        assert_eq!(back.url, "{{base}}/items/1?tag=a+b&tag=%C3%B1");
        assert_eq!(back.headers[0], KeyValue::new("X-Quote", "it's \"q\""));
        assert_eq!(back.body, def.body);
        assert_eq!(back.auth, def.auth, "templates are not secrets");
        assert_eq!(back.timeout_secs, 12);
    }

    #[test]
    fn export_omits_literal_secrets_by_default() {
        let def = RequestDefinition {
            url: "https://h/".into(),
            headers: vec![KeyValue::new("X-Api-Key", "k-123")],
            auth: Auth::Basic {
                username: "ana".into(),
                password: "hunter2".into(),
            },
            ..RequestDefinition::default()
        };
        let cmd = export_posix(&def, false);
        assert!(!cmd.contains("hunter2") && !cmd.contains("k-123"), "{cmd}");
        assert!(cmd.contains("<omitted>"));
        let cmd = export_posix(&def, true);
        assert!(cmd.contains("hunter2") && cmd.contains("k-123"));
    }
}
