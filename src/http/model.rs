//! Persistable request definitions (what the user edits and saves).
//!
//! Nothing here is resolved or validated: `{{variables}}` stay as written.
//! See `resolve.rs` for turning a definition into a sendable request.

use serde::{Deserialize, Serialize};

use super::request::Method;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyValue {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub key: String,
    #[serde(default)]
    pub value: String,
}

fn yes() -> bool {
    true
}

impl KeyValue {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            enabled: true,
            key: key.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BodyMode {
    #[default]
    None,
    Text,
    Json,
}

impl BodyMode {
    pub const ALL: [BodyMode; 3] = [BodyMode::None, BodyMode::Json, BodyMode::Text];

    pub fn label(self) -> &'static str {
        match self {
            BodyMode::None => "None",
            BodyMode::Text => "Text",
            BodyMode::Json => "JSON",
        }
    }

    pub fn content_type(self) -> Option<&'static str> {
        match self {
            BodyMode::None => None,
            BodyMode::Text => Some("text/plain; charset=utf-8"),
            BodyMode::Json => Some("application/json"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Body {
    #[serde(default)]
    pub mode: BodyMode,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Auth {
    #[default]
    None,
    Bearer {
        #[serde(default)]
        token: String,
    },
    Basic {
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
    },
}

impl Auth {
    pub fn label(&self) -> &'static str {
        match self {
            Auth::None => "None",
            Auth::Bearer { .. } => "Bearer token",
            Auth::Basic { .. } => "Basic auth",
        }
    }
}

/// A `{{name}}` variable. Secret values are never written to project files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub secret: bool,
    /// Opt-in: keep the secret in the OS credential store between sessions.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remember: bool,
}

impl Variable {
    pub fn plain(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            secret: false,
            remember: false,
        }
    }

    pub fn secret(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            secret: true,
            ..Self::plain(name, value)
        }
    }
}

pub const DEFAULT_TIMEOUT_SECS: u32 = 30;

fn default_timeout() -> u32 {
    DEFAULT_TIMEOUT_SECS
}

fn is_default_timeout(v: &u32) -> bool {
    *v == DEFAULT_TIMEOUT_SECS
}

/// A short random-enough id for requests in a collection.
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}", nanos ^ (n.wrapping_mul(0x9E37_79B9_7F4A_7C15)))
}

/// An editable HTTP request, as saved in collections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestDefinition {
    /// Stable id inside its collection (assigned when missing).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub name: String,
    pub method: Method,
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body: Body,
    #[serde(default)]
    pub auth: Auth,
    /// Request-local variables (highest precedence).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
    #[serde(
        default = "default_timeout",
        skip_serializing_if = "is_default_timeout"
    )]
    pub timeout_secs: u32,
}

impl Default for RequestDefinition {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "New request".to_owned(),
            method: Method::Get,
            url: String::new(),
            params: Vec::new(),
            headers: Vec::new(),
            body: Body::default(),
            auth: Auth::None,
            variables: Vec::new(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }
}
