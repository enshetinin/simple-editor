use std::error::Error as _;
use std::fmt;
use std::time::{Duration, Instant};

use super::request::PreparedRequest;

/// Default limit for the in-memory response preview (decompressed bytes).
pub const DEFAULT_PREVIEW_LIMIT: usize = 5 * 1024 * 1024;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_REDIRECTS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientConfig {
    pub timeout: Duration,
    pub preview_limit: usize,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            preview_limit: DEFAULT_PREVIEW_LIMIT,
        }
    }
}

pub fn build_client(config: &ClientConfig) -> Result<reqwest::Client, HttpError> {
    reqwest::Client::builder()
        .timeout(config.timeout)
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .user_agent(concat!("SimpleEditor/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| HttpError::Other(error_chain(&e)))
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub reason: Option<&'static str>,
    pub version: String,
    pub final_url: String,
    pub headers: Vec<(String, String)>,
    /// Body bytes kept for preview, at most the preview limit.
    pub body: Vec<u8>,
    /// Body decoded as UTF-8, or `None` if it is binary / not UTF-8.
    pub body_text: Option<String>,
    /// True when the body was longer than the preview limit and reading stopped.
    pub truncated: bool,
    /// Decompressed body bytes read from the network.
    pub bytes_received: u64,
    /// Time from sending until the preview was fully read (or truncated).
    pub elapsed: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    Timeout,
    Dns(String),
    Tls(String),
    Connect(String),
    Redirect(String),
    /// The connection broke while reading the body.
    Body(String),
    Other(String),
}

impl HttpError {
    pub fn title(&self) -> &'static str {
        match self {
            HttpError::Timeout => "Timed out",
            HttpError::Dns(_) => "DNS lookup failed",
            HttpError::Tls(_) => "TLS / certificate error",
            HttpError::Connect(_) => "Could not connect",
            HttpError::Redirect(_) => "Redirect error",
            HttpError::Body(_) => "Connection interrupted while reading the response",
            HttpError::Other(_) => "Request failed",
        }
    }

    pub fn hint(&self) -> &'static str {
        match self {
            HttpError::Timeout => {
                "The server did not finish responding in time. Try again or check the server."
            }
            HttpError::Dns(_) => "Check the host name and your network connection.",
            HttpError::Tls(_) => {
                "The server certificate could not be verified. Check the URL and the server certificate."
            }
            HttpError::Connect(_) => {
                "Check that the server is running and the host/port are correct."
            }
            HttpError::Redirect(_) => {
                "The server redirected too many times or to an invalid location."
            }
            HttpError::Body(_) => "Try again; the server closed the connection early.",
            HttpError::Other(_) => "Check the request and try again.",
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            HttpError::Timeout => "",
            HttpError::Dns(d)
            | HttpError::Tls(d)
            | HttpError::Connect(d)
            | HttpError::Redirect(d)
            | HttpError::Body(d)
            | HttpError::Other(d) => d,
        }
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.title())?;
        if !self.detail().is_empty() {
            write!(f, ": {}", self.detail())?;
        }
        Ok(())
    }
}

fn error_chain(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        let s_text = s.to_string();
        if !out.contains(&s_text) {
            out.push_str(": ");
            out.push_str(&s_text);
        }
        source = s.source();
    }
    out
}

fn classify(e: &reqwest::Error, reading_body: bool) -> HttpError {
    let detail = error_chain(e);
    if e.is_timeout() {
        return HttpError::Timeout;
    }
    if e.is_dns() {
        return HttpError::Dns(detail);
    }
    if e.is_redirect() {
        return HttpError::Redirect(detail);
    }
    let lower = detail.to_ascii_lowercase();
    if lower.contains("certificate") || lower.contains("tls") || lower.contains("handshake") {
        return HttpError::Tls(detail);
    }
    if e.is_connect() {
        return HttpError::Connect(detail);
    }
    if reading_body || e.is_body() || e.is_decode() {
        return HttpError::Body(detail);
    }
    HttpError::Other(detail)
}

/// Sends a request and reads at most `preview_limit` decompressed body bytes.
///
/// HTTP 4xx/5xx are successful responses here, not errors.
pub async fn execute(
    client: &reqwest::Client,
    request: PreparedRequest,
    preview_limit: usize,
) -> Result<HttpResponse, HttpError> {
    let started = Instant::now();
    let mut builder = client.request(request.method.to_reqwest(), request.url);
    if let Some((kind, body)) = request.body {
        builder = builder
            .header(reqwest::header::CONTENT_TYPE, kind.content_type())
            .body(body);
    }
    let mut response = builder.send().await.map_err(|e| classify(&e, false))?;

    let status = response.status();
    let version = format!("{:?}", response.version());
    let final_url = response.url().to_string();
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_owned(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();

    let mut body = Vec::new();
    let mut bytes_received = 0u64;
    let mut truncated = false;
    while let Some(chunk) = response.chunk().await.map_err(|e| classify(&e, true))? {
        bytes_received += chunk.len() as u64;
        let room = preview_limit - body.len();
        if chunk.len() > room {
            body.extend_from_slice(&chunk[..room]);
            truncated = true;
            // Stop reading: dropping the response aborts the transfer.
            break;
        }
        body.extend_from_slice(&chunk);
    }
    drop(response);
    let elapsed = started.elapsed();

    let body_text = decode_preview(&body, truncated);
    Ok(HttpResponse {
        status: status.as_u16(),
        reason: status.canonical_reason(),
        version,
        final_url,
        headers,
        body,
        body_text,
        truncated,
        bytes_received,
        elapsed,
    })
}

fn decode_preview(body: &[u8], truncated: bool) -> Option<String> {
    if body.contains(&0) {
        return None;
    }
    match std::str::from_utf8(body) {
        Ok(s) => Some(s.to_owned()),
        // A truncated preview may cut a multi-byte character at the end.
        Err(e) if truncated && e.error_len().is_none() => {
            Some(String::from_utf8_lossy(&body[..e.valid_up_to()]).into_owned())
        }
        Err(_) => None,
    }
}
