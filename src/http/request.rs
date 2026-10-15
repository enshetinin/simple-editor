use std::fmt;

use reqwest::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl Method {
    pub const ALL: [Method; 7] = [
        Method::Get,
        Method::Post,
        Method::Put,
        Method::Patch,
        Method::Delete,
        Method::Head,
        Method::Options,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
        }
    }

    pub(crate) fn to_reqwest(self) -> reqwest::Method {
        match self {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Put => reqwest::Method::PUT,
            Method::Patch => reqwest::Method::PATCH,
            Method::Delete => reqwest::Method::DELETE,
            Method::Head => reqwest::Method::HEAD,
            Method::Options => reqwest::Method::OPTIONS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Text,
    Json,
}

impl BodyKind {
    pub fn label(self) -> &'static str {
        match self {
            BodyKind::Text => "Text",
            BodyKind::Json => "JSON",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            BodyKind::Text => "text/plain; charset=utf-8",
            BodyKind::Json => "application/json",
        }
    }
}

/// What the user typed. Nothing here is trusted or interpreted as a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestDraft {
    pub method: Method,
    pub url: String,
    pub body: String,
    pub body_kind: BodyKind,
}

impl Default for RequestDraft {
    fn default() -> Self {
        Self {
            method: Method::Get,
            url: String::new(),
            body: String::new(),
            body_kind: BodyKind::Json,
        }
    }
}

/// A validated request, ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRequest {
    pub method: Method,
    pub url: Url,
    pub body: Option<(BodyKind, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    EmptyUrl,
    InvalidUrl(String),
    UnsupportedScheme(String),
    MissingHost,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::EmptyUrl => {
                write!(f, "URL: enter a URL such as http://localhost:8080/")
            }
            ValidationError::InvalidUrl(e) => write!(f, "URL: {e}"),
            ValidationError::UnsupportedScheme(s) => {
                write!(
                    f,
                    "URL: scheme \"{s}\" is not supported; use http:// or https://"
                )
            }
            ValidationError::MissingHost => write!(f, "URL: the host is missing"),
        }
    }
}

impl RequestDraft {
    pub fn prepare(&self) -> Result<PreparedRequest, ValidationError> {
        let raw = self.url.trim();
        if raw.is_empty() {
            return Err(ValidationError::EmptyUrl);
        }
        let url = Url::parse(raw).map_err(|e| match e {
            url::ParseError::RelativeUrlWithoutBase => {
                ValidationError::InvalidUrl("add the scheme, e.g. http:// or https://".to_owned())
            }
            other => ValidationError::InvalidUrl(other.to_string()),
        })?;
        match url.scheme() {
            "http" | "https" => {}
            other => return Err(ValidationError::UnsupportedScheme(other.to_owned())),
        }
        if url.host_str().is_none_or(str::is_empty) {
            return Err(ValidationError::MissingHost);
        }
        let body = (!self.body.is_empty()).then(|| (self.body_kind, self.body.clone()));
        Ok(PreparedRequest {
            method: self.method,
            url,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(url: &str) -> RequestDraft {
        RequestDraft {
            url: url.to_owned(),
            ..RequestDraft::default()
        }
    }

    #[test]
    fn validates_scheme_and_host() {
        assert_eq!(draft("  ").prepare(), Err(ValidationError::EmptyUrl));
        assert!(matches!(
            draft("localhost:8080/x").prepare(),
            Err(ValidationError::UnsupportedScheme(_))
        ));
        assert!(matches!(
            draft("/relative").prepare(),
            Err(ValidationError::InvalidUrl(_))
        ));
        assert!(matches!(
            draft("file:///etc/passwd").prepare(),
            Err(ValidationError::UnsupportedScheme(_))
        ));
        // Shell metacharacters are just URL characters, never executed.
        let ok = draft("http://127.0.0.1:1/a?q=$(rm -rf ~)&x=`id`")
            .prepare()
            .unwrap();
        assert_eq!(ok.url.host_str(), Some("127.0.0.1"));
        assert!(ok.body.is_none());
    }
}
