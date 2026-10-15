//! What counts as a secret, and how definitions are cleaned before being
//! written to project files, exports or history.
//!
//! Values written as `{{variables}}` are not secrets: the secret lives in the
//! variable, whose value is never written to project files.

use super::model::{Auth, KeyValue, RequestDefinition};
use super::vars;

const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "api-key",
    "x-auth-token",
    "x-csrf-token",
    "x-xsrf-token",
];

const SENSITIVE_WORDS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "pwd",
    "apikey",
    "api_key",
    "api-key",
    "auth",
    "session",
    "signature",
    "credential",
    "private",
];

const SENSITIVE_PARAMS: &[&str] = &[
    "key",
    "sig",
    "code",
    "pass",
    "access_token",
    "client_secret",
];

pub fn is_sensitive_header(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    SENSITIVE_HEADERS.contains(&n.as_str()) || SENSITIVE_WORDS.iter().any(|w| n.contains(w))
}

pub fn is_sensitive_param(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    SENSITIVE_PARAMS.contains(&n.as_str()) || SENSITIVE_WORDS.iter().any(|w| n.contains(w))
}

/// A value is safe to persist if every non-empty part is a `{{variable}}`
/// reference or it contains no literal text at all.
fn literal(value: &str) -> bool {
    vars::parse(value).iter().any(|s| match s {
        vars::Segment::Text(t) => !t.trim().is_empty(),
        vars::Segment::Var(_) => false,
    })
}

/// For a header value like `Bearer abc`, the scheme is not the secret.
fn literal_credential(value: &str) -> bool {
    let rest = value
        .trim()
        .strip_prefix("Bearer ")
        .or_else(|| value.trim().strip_prefix("Basic "))
        .unwrap_or(value);
    literal(rest)
}

pub const REDACTED: &str = "[redacted]";

/// Removes literal credentials from a definition: secret request variables,
/// literal Auth values and literal values of sensitive headers/params.
/// Returns the cleaned definition and the fields that were emptied.
pub fn strip_secrets(def: &RequestDefinition) -> (RequestDefinition, Vec<String>) {
    let mut out = def.clone();
    let mut omitted = Vec::new();
    for v in &mut out.variables {
        if v.secret && !v.value.is_empty() {
            v.value.clear();
            omitted.push(format!("Variables › {}", v.name));
        }
    }
    match &mut out.auth {
        Auth::None => {}
        Auth::Bearer { token } => {
            if literal(token) {
                token.clear();
                omitted.push("Auth › Token".to_owned());
            }
        }
        Auth::Basic { password, .. } => {
            if literal(password) {
                password.clear();
                omitted.push("Auth › Password".to_owned());
            }
        }
    }
    let mut clean = |list: &mut Vec<KeyValue>, section: &str, sensitive: fn(&str) -> bool| {
        for kv in list.iter_mut() {
            if sensitive(&kv.key) && literal_credential(&kv.value) {
                kv.value.clear();
                omitted.push(format!("{section} › {}", kv.key));
            }
        }
    };
    clean(&mut out.headers, "Headers", is_sensitive_header);
    clean(&mut out.params, "Params", is_sensitive_param);
    let (url, url_changed) = sanitize_url(&out.url);
    if url_changed {
        out.url = url;
        omitted.push("URL".to_owned());
    }
    (out, omitted)
}

/// Redacts a URL template for display/history: a userinfo password and the
/// literal values of sensitive query parameters. Returns whether it changed.
pub fn sanitize_url(url: &str) -> (String, bool) {
    let mut out = url.to_owned();
    let mut changed = false;
    // scheme://user:password@host
    if let Some(scheme_end) = out.find("://") {
        let auth_start = scheme_end + 3;
        let authority_end = out[auth_start..]
            .find(['/', '?', '#'])
            .map_or(out.len(), |i| auth_start + i);
        if let Some(at) = out[auth_start..authority_end].rfind('@') {
            let userinfo = &out[auth_start..auth_start + at];
            if let Some((user, pass)) = userinfo.split_once(':')
                && literal(pass)
            {
                let replaced = format!("{user}:{REDACTED}");
                out.replace_range(auth_start..auth_start + at, &replaced);
                changed = true;
            }
        }
    }
    if let Some(q) = out.find('?') {
        let (base, query) = out.split_at(q + 1);
        let (query, fragment) = match query.split_once('#') {
            Some((q, f)) => (q, Some(f)),
            None => (query, None),
        };
        let parts: Vec<String> = query
            .split('&')
            .map(|pair| match pair.split_once('=') {
                Some((k, v)) if is_sensitive_param(k) && literal(v) => {
                    changed = true;
                    format!("{k}={REDACTED}")
                }
                _ => pair.to_owned(),
            })
            .collect();
        let mut rebuilt = format!("{base}{}", parts.join("&"));
        if let Some(f) = fragment {
            rebuilt.push('#');
            rebuilt.push_str(f);
        }
        out = rebuilt;
    }
    (out, changed)
}

/// What history keeps of a request: no body, no auth values, no literal
/// secrets in URL/params/headers. Templates (`{{token}}`) are kept.
pub fn for_history(def: &RequestDefinition) -> RequestDefinition {
    let (mut clean, _) = strip_secrets(def);
    // Make removals visible instead of looking like empty values.
    let mark = |clean: &mut [KeyValue], original: &[KeyValue]| {
        for (c, o) in clean.iter_mut().zip(original) {
            if c.value.is_empty() && !o.value.is_empty() {
                c.value = REDACTED.to_owned();
            }
        }
    };
    mark(&mut clean.headers, &def.headers);
    mark(&mut clean.params, &def.params);
    clean.body.text.clear();
    clean.variables.clear();
    clean.auth = match clean.auth {
        Auth::None => Auth::None,
        Auth::Bearer { token } => Auth::Bearer {
            token: if literal(&token) || token.is_empty() {
                String::new()
            } else {
                token
            },
        },
        Auth::Basic { username, .. } => Auth::Basic {
            username,
            password: String::new(),
        },
    };
    clean
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::model::{Body, BodyMode, Variable};

    #[test]
    fn literal_credentials_are_removed_templates_kept() {
        let def = RequestDefinition {
            url: "https://u:pw@h/x?api_key=abc&page=2&token={{t}}#frag".into(),
            params: vec![
                KeyValue::new("access_token", "zzz"),
                KeyValue::new("q", "x"),
            ],
            headers: vec![
                KeyValue::new("Authorization", "Bearer abc"),
                KeyValue::new("X-Api-Key", "{{key}}"),
                KeyValue::new("Accept", "application/json"),
            ],
            auth: Auth::Basic {
                username: "ana".into(),
                password: "pw".into(),
            },
            variables: vec![Variable::secret("t", "123"), Variable::plain("p", "1")],
            ..RequestDefinition::default()
        };
        let (clean, omitted) = strip_secrets(&def);
        assert_eq!(
            clean.url,
            "https://u:[redacted]@h/x?api_key=[redacted]&page=2&token={{t}}#frag"
        );
        assert_eq!(clean.params[0].value, "");
        assert_eq!(clean.params[1].value, "x");
        assert_eq!(clean.headers[0].value, "");
        assert_eq!(clean.headers[1].value, "{{key}}");
        assert_eq!(clean.headers[2].value, "application/json");
        assert_eq!(
            clean.auth,
            Auth::Basic {
                username: "ana".into(),
                password: String::new()
            }
        );
        assert_eq!(clean.variables[0].value, "");
        assert_eq!(clean.variables[1].value, "1");
        assert_eq!(omitted.len(), 5, "{omitted:?}"); // URL counts once

        let serialized = serde_json::to_string(&clean).unwrap();
        for secret in ["abc", "zzz", "\"pw\"", "123", "u:pw"] {
            assert!(!serialized.contains(secret), "{secret} in {serialized}");
        }
    }

    #[test]
    fn history_has_no_body_or_auth_values() {
        let def = RequestDefinition {
            url: "{{base}}/login".into(),
            body: Body {
                mode: BodyMode::Json,
                text: "{\"password\":\"x\"}".into(),
            },
            auth: Auth::Bearer {
                token: "{{token}}".into(),
            },
            headers: vec![KeyValue::new("Cookie", "sid=1")],
            ..RequestDefinition::default()
        };
        let h = for_history(&def);
        assert!(h.body.text.is_empty());
        assert_eq!(h.headers[0].value, REDACTED);
        assert_eq!(
            h.auth,
            Auth::Bearer {
                token: "{{token}}".into()
            }
        );
    }
}
