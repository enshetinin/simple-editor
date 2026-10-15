//! Turns a `RequestDefinition` into a `ResolvedRequest`: expands variables,
//! validates every field and builds the exact headers and body to send.
//! Nothing is sent if any field fails; each error names its field.

use std::fmt;
use std::time::Duration;

use reqwest::Url;
use reqwest::header::{HeaderName, HeaderValue};

use super::model::{Auth, BodyMode, RequestDefinition};
use super::request::{Method, parse_http_url};
use super::vars::{Scopes, expand};

/// A request ready to send. Header order and repeats are preserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRequest {
    pub method: Method,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
    /// Secret values inserted from variables, to redact from any display.
    pub secrets: Vec<String>,
}

impl ResolvedRequest {
    /// Minimal GET for tests and tools.
    pub fn get(url: &str) -> Result<Self, String> {
        Ok(Self {
            method: Method::Get,
            url: parse_http_url(url)?,
            headers: Vec::new(),
            body: None,
            timeout: Duration::from_secs(super::model::DEFAULT_TIMEOUT_SECS as u64),
            secrets: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    /// Human-readable location, e.g. `Headers › X-Api-Key`.
    pub field: String,
    pub message: String,
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

struct Ctx<'a> {
    scopes: Scopes<'a>,
    errors: Vec<FieldError>,
    secrets: Vec<String>,
}

impl Ctx<'_> {
    fn expand(&mut self, field: &str, template: &str) -> Option<String> {
        match expand(template, &self.scopes) {
            Ok(e) => {
                self.secrets.extend(e.secrets);
                Some(e.text)
            }
            Err(missing) => {
                for (name, reason) in missing {
                    self.errors.push(FieldError {
                        field: field.to_owned(),
                        message: format!("{{{{{name}}}}}: {reason}"),
                    });
                }
                None
            }
        }
    }

    fn error(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.errors.push(FieldError {
            field: field.into(),
            message: message.into(),
        });
    }
}

pub fn resolve(
    def: &RequestDefinition,
    environment: &[super::model::Variable],
    project: &[super::model::Variable],
) -> Result<ResolvedRequest, Vec<FieldError>> {
    let mut cx = Ctx {
        scopes: Scopes {
            request: &def.variables,
            environment,
            project,
        },
        errors: Vec::new(),
        secrets: Vec::new(),
    };

    // URL and query parameters (repeated keys kept, in order).
    let mut url = cx
        .expand("URL", &def.url)
        .and_then(|raw| match parse_http_url(&raw) {
            Ok(u) => Some(u),
            Err(e) => {
                cx.error("URL", e);
                None
            }
        });
    let mut params = Vec::new();
    for (i, p) in def.params.iter().enumerate() {
        if !p.enabled || (p.key.is_empty() && p.value.is_empty()) {
            continue;
        }
        let field = format!("Params › {}", label(&p.key, i));
        if p.key.trim().is_empty() {
            cx.error(&field, "the name is empty");
            continue;
        }
        let k = cx.expand(&field, &p.key);
        let v = cx.expand(&field, &p.value);
        if let (Some(k), Some(v)) = (k, v) {
            params.push((k, v));
        }
    }
    if let Some(u) = url.as_mut()
        && !params.is_empty()
    {
        let mut pairs = u.query_pairs_mut();
        for (k, v) in &params {
            pairs.append_pair(k, v);
        }
    }

    // Headers.
    let mut headers: Vec<(String, String)> = Vec::new();
    for (i, h) in def.headers.iter().enumerate() {
        if !h.enabled || (h.key.is_empty() && h.value.is_empty()) {
            continue;
        }
        let field = format!("Headers › {}", label(&h.key, i));
        let k = cx.expand(&field, h.key.trim());
        let v = cx.expand(&field, &h.value);
        let (Some(k), Some(v)) = (k, v) else { continue };
        if HeaderName::from_bytes(k.as_bytes()).is_err() {
            cx.error(&field, "invalid header name");
            continue;
        }
        if HeaderValue::from_str(&v).is_err() {
            cx.error(
                &field,
                "invalid header value (control characters or line breaks)",
            );
            continue;
        }
        headers.push((k, v));
    }
    let has_header = |headers: &[(String, String)], name: &str| {
        headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
    };

    // Auth.
    match &def.auth {
        Auth::None => {}
        auth if has_header(&headers, "authorization") => {
            cx.error(
                "Auth",
                format!(
                    "{} is set and Headers also contains Authorization; keep only one",
                    auth.label()
                ),
            );
        }
        Auth::Bearer { token } => {
            if let Some(t) = cx.expand("Auth › Token", token) {
                if t.trim().is_empty() {
                    cx.error("Auth › Token", "the token is empty");
                } else if HeaderValue::from_str(&t).is_err() {
                    cx.error("Auth › Token", "the token contains invalid characters");
                } else {
                    headers.push(("Authorization".to_owned(), format!("Bearer {t}")));
                }
            }
        }
        Auth::Basic { username, password } => {
            let u = cx.expand("Auth › Username", username);
            let p = cx.expand("Auth › Password", password);
            if let (Some(u), Some(p)) = (u, p) {
                if u.contains(':') {
                    cx.error("Auth › Username", "Basic auth usernames cannot contain ':'");
                } else {
                    let encoded = base64(format!("{u}:{p}").as_bytes());
                    cx.secrets.push(encoded.clone());
                    headers.push(("Authorization".to_owned(), format!("Basic {encoded}")));
                }
            }
        }
    }

    // Body.
    let body = match def.body.mode {
        BodyMode::None => None,
        mode => cx.expand("Body", &def.body.text).and_then(|text| {
            if mode == BodyMode::Json
                && !text.trim().is_empty()
                && let Err(e) = serde_json::from_str::<serde::de::IgnoredAny>(&text)
            {
                cx.error(
                    "Body",
                    format!("invalid JSON at line {}, column {}", e.line(), e.column()),
                );
                return None;
            }
            if let Some(ct) = mode.content_type()
                && !has_header(&headers, "content-type")
            {
                headers.push(("Content-Type".to_owned(), ct.to_owned()));
            }
            Some(text.into_bytes())
        }),
    };

    if def.timeout_secs == 0 {
        cx.error("Settings › Timeout", "must be at least 1 second");
    }

    match url {
        Some(url) if cx.errors.is_empty() => Ok(ResolvedRequest {
            method: def.method,
            url,
            headers,
            body,
            timeout: Duration::from_secs(def.timeout_secs.max(1) as u64),
            secrets: cx.secrets,
        }),
        _ => Err(cx.errors),
    }
}

fn label(key: &str, index: usize) -> String {
    if key.trim().is_empty() {
        format!("row {}", index + 1)
    } else {
        key.trim().to_owned()
    }
}

/// Standard base64 (RFC 4648) with padding.
pub fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Decodes standard base64; `None` if invalid.
pub fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let clean: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !clean.len().is_multiple_of(4) {
        return None;
    }
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 {
            return None;
        }
        let mut n = 0u32;
        for &c in &chunk[..4 - pad] {
            n = (n << 6) | val(c)?;
        }
        n <<= 6 * pad as u32;
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::model::{Body, KeyValue, Variable};

    fn def(url: &str) -> RequestDefinition {
        RequestDefinition {
            url: url.to_owned(),
            ..RequestDefinition::default()
        }
    }

    #[test]
    fn base64_round_trip() {
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64("ñ:🔑".as_bytes()), "w7E68J+UkQ==");
        for s in ["", "a", "ab", "abc", "user:pässwörd"] {
            assert_eq!(base64_decode(&base64(s.as_bytes())).unwrap(), s.as_bytes());
        }
        assert!(base64_decode("@@@=").is_none());
    }

    #[test]
    fn repeated_params_and_headers_keep_order_and_encoding() {
        let mut d = def("{{base}}/search?fixed=1");
        d.params = vec![
            KeyValue::new("tag", "a b"),
            KeyValue::new("tag", "ñ&x"),
            KeyValue {
                enabled: false,
                ..KeyValue::new("off", "1")
            },
        ];
        d.headers = vec![KeyValue::new("X-Multi", "1"), KeyValue::new("X-Multi", "2")];
        let env = [Variable::plain("base", "http://localhost:9")];
        let r = resolve(&d, &env, &[]).unwrap();
        assert_eq!(
            r.url.as_str(),
            "http://localhost:9/search?fixed=1&tag=a+b&tag=%C3%B1%26x"
        );
        assert_eq!(
            r.headers,
            vec![
                ("X-Multi".to_owned(), "1".to_owned()),
                ("X-Multi".to_owned(), "2".to_owned())
            ]
        );
    }

    #[test]
    fn auth_body_and_content_type() {
        let mut d = def("http://h/");
        d.auth = Auth::Basic {
            username: "{{user}}".into(),
            password: "{{pass}}".into(),
        };
        d.body = Body {
            mode: BodyMode::Json,
            text: "{\"a\": 1}".into(),
        };
        let env = [
            Variable::plain("user", "ana"),
            Variable::secret("pass", "pw"),
        ];
        let r = resolve(&d, &env, &[]).unwrap();
        assert!(
            r.headers
                .contains(&("Authorization".into(), "Basic YW5hOnB3".into()))
        );
        assert!(
            r.headers
                .contains(&("Content-Type".into(), "application/json".into()))
        );
        assert_eq!(r.body.as_deref(), Some(&b"{\"a\": 1}"[..]));
        assert!(r.secrets.contains(&"pw".to_owned()));

        // A user Content-Type wins.
        d.headers = vec![KeyValue::new("content-type", "application/vnd.x+json")];
        let r = resolve(&d, &env, &[]).unwrap();
        assert_eq!(
            r.headers
                .iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                .count(),
            1
        );
    }

    #[test]
    fn errors_name_each_field_and_block_sending() {
        let mut d = def("{{base}}/x");
        d.params = vec![KeyValue::new("", "orphan")];
        d.headers = vec![
            KeyValue::new("Bad Header", "v"),
            KeyValue::new("X-Token", "{{missing}}"),
            KeyValue::new("Authorization", "x"),
        ];
        d.auth = Auth::Bearer {
            token: "{{token}}".into(),
        };
        d.body = Body {
            mode: BodyMode::Json,
            text: "{oops".into(),
        };
        let env = [Variable::secret("token", "")];
        let errors = resolve(&d, &env, &[]).unwrap_err();
        let fields: Vec<_> = errors.iter().map(|e| e.field.as_str()).collect();
        assert_eq!(
            fields,
            vec![
                "URL",
                "Params › row 1",
                "Headers › Bad Header",
                "Headers › X-Token",
                "Auth",
                "Body"
            ],
            "{errors:#?}"
        );
        assert!(errors[0].message.contains("{{base}}"));
        assert!(errors[3].message.contains("{{missing}}: unknown variable"));
    }
}
