use std::error::Error as _;
use std::fmt;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::json;
use super::resolve::ResolvedRequest;

/// Default limit for the in-memory response preview (decompressed bytes).
pub const DEFAULT_PREVIEW_LIMIT: usize = 5 * 1024 * 1024;
/// Default limit for "save response to file" downloads.
pub const DEFAULT_DOWNLOAD_LIMIT: u64 = 1024 * 1024 * 1024;
pub const MAX_REDIRECTS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    pub preview_limit: usize,
    /// Extra trusted root certificates (PEM). Added to the system roots;
    /// certificate verification is never disabled.
    pub extra_roots_pem: Vec<Vec<u8>>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            preview_limit: DEFAULT_PREVIEW_LIMIT,
            extra_roots_pem: Vec::new(),
        }
    }
}

pub fn build_client(config: &ClientConfig) -> Result<reqwest::Client, HttpError> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .user_agent(concat!("SimpleEditor/", env!("CARGO_PKG_VERSION")));
    let mut roots = Vec::new();
    for pem in &config.extra_roots_pem {
        let certs = reqwest::Certificate::from_pem_bundle(pem)
            .map_err(|e| HttpError::Other(format!("invalid trusted certificate: {e}")))?;
        roots.extend(certs);
    }
    if !roots.is_empty() {
        builder = builder.tls_certs_merge(roots);
    }
    builder
        .build()
        .map_err(|e| HttpError::Other(error_chain(&e)))
}

/// Where the response body goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodySink {
    /// Keep up to the preview limit in memory.
    Preview,
    /// Stream to a file (written atomically) up to `max_bytes`.
    File { path: PathBuf, max_bytes: u64 },
}

/// Text prepared for a virtualized view: byte ranges of each line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextLines {
    pub text: String,
    pub lines: Vec<std::ops::Range<usize>>,
    /// Longest line in bytes.
    pub longest: usize,
}

impl TextLines {
    pub fn new(text: String) -> Self {
        let mut lines = Vec::new();
        let mut start = 0;
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                lines.push(start..i);
                start = i + 1;
            }
        }
        if start < text.len() || lines.is_empty() {
            lines.push(start..text.len());
        }
        let longest = lines.iter().map(|r| r.len()).max().unwrap_or(0);
        Self {
            text,
            lines,
            longest,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonView {
    NotJson,
    /// Valid JSON, pretty-printed on the worker.
    Pretty(TextLines),
    /// Declared or looks like JSON but does not parse.
    Invalid(String),
    /// JSON body cut at the preview limit; pretty view unavailable.
    Truncated,
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
    /// `body_text` split into lines for display.
    pub body_lines: TextLines,
    pub json: JsonView,
    /// True when the body was longer than the preview limit and reading stopped.
    pub truncated: bool,
    /// Decompressed body bytes read from the network.
    pub bytes_received: u64,
    /// Time from sending until the body was fully read (or truncated).
    pub elapsed: Duration,
    /// Set when the body was saved to a file.
    pub saved_to: Option<PathBuf>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
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
    /// Writing a downloaded body failed (disk full, permissions…).
    Download(String),
    /// The download exceeded the configured limit.
    TooLarge(u64),
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
            HttpError::Download(_) => "Could not save the response",
            HttpError::TooLarge(_) => "Download larger than the limit",
            HttpError::Other(_) => "Request failed",
        }
    }

    pub fn hint(&self) -> &'static str {
        match self {
            HttpError::Timeout => {
                "The server did not finish responding in time. Increase the timeout in Settings or check the server."
            }
            HttpError::Dns(_) => "Check the host name and your network connection.",
            HttpError::Tls(_) => {
                "The server certificate could not be verified. Check the URL and the server certificate."
            }
            HttpError::Connect(_) => {
                "Check that the server is running and the host/port are correct."
            }
            HttpError::Redirect(_) => {
                "The server redirected too many times (max 10) or to an invalid location."
            }
            HttpError::Body(_) => "Try again; the server closed the connection early.",
            HttpError::Download(_) => {
                "Nothing was written to the chosen file. Check free space and permissions."
            }
            HttpError::TooLarge(_) => {
                "The partial file was removed. Raise the download limit in Settings to save it."
            }
            HttpError::Other(_) => "Check the request and try again.",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            HttpError::Timeout => String::new(),
            HttpError::TooLarge(limit) => format!("limit: {limit} bytes"),
            HttpError::Dns(d)
            | HttpError::Tls(d)
            | HttpError::Connect(d)
            | HttpError::Redirect(d)
            | HttpError::Body(d)
            | HttpError::Download(d)
            | HttpError::Other(d) => d.clone(),
        }
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.title())?;
        let detail = self.detail();
        if !detail.is_empty() {
            write!(f, ": {detail}")?;
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

/// Replaces secret values in `text` (used for error messages).
pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for s in secrets.iter().filter(|s| s.len() >= 3) {
        out = out.replace(s.as_str(), "•••");
    }
    out
}

/// Sends a request. HTTP 4xx/5xx are successful responses here, not errors.
pub async fn execute(
    client: &reqwest::Client,
    request: ResolvedRequest,
    sink: BodySink,
    preview_limit: usize,
) -> Result<HttpResponse, HttpError> {
    let secrets = request.secrets.clone();
    execute_inner(client, request, sink, preview_limit)
        .await
        .map_err(|e| match e {
            HttpError::Other(d) => HttpError::Other(redact(&d, &secrets)),
            HttpError::Connect(d) => HttpError::Connect(redact(&d, &secrets)),
            other => other,
        })
}

async fn execute_inner(
    client: &reqwest::Client,
    request: ResolvedRequest,
    sink: BodySink,
    preview_limit: usize,
) -> Result<HttpResponse, HttpError> {
    let started = Instant::now();
    let mut builder = client
        .request(request.method.to_reqwest(), request.url)
        .timeout(request.timeout);
    for (k, v) in &request.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    if let Some(body) = request.body {
        builder = builder.body(body);
    }
    let mut response = builder.send().await.map_err(|e| classify(&e, false))?;

    let status = response.status();
    let version = format!("{:?}", response.version());
    let final_url = response.url().to_string();
    let headers: Vec<(String, String)> = response
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
    let mut saved_to = None;
    match sink {
        BodySink::Preview => {
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
        }
        BodySink::File { path, max_bytes } => {
            let dir = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            // Partial data goes to a temporary file that is removed on any
            // error or cancellation (drop), and renamed only when complete.
            let tmp = tempfile::Builder::new()
                .prefix(".se-download-")
                .tempfile_in(&dir)
                .map_err(|e| HttpError::Download(e.to_string()))?;
            let mut writer = std::io::BufWriter::with_capacity(256 * 1024, tmp);
            while let Some(chunk) = response.chunk().await.map_err(|e| classify(&e, true))? {
                bytes_received += chunk.len() as u64;
                if bytes_received > max_bytes {
                    return Err(HttpError::TooLarge(max_bytes));
                }
                writer
                    .write_all(&chunk)
                    .map_err(|e| HttpError::Download(e.to_string()))?;
            }
            let tmp = writer
                .into_inner()
                .map_err(|e| HttpError::Download(e.into_error().to_string()))?;
            tmp.as_file()
                .sync_all()
                .map_err(|e| HttpError::Download(e.to_string()))?;
            tmp.persist(&path)
                .map_err(|e| HttpError::Download(e.error.to_string()))?;
            saved_to = Some(path);
        }
    }
    drop(response);
    let elapsed = started.elapsed();

    // Decoding and JSON formatting can be expensive: keep them off the
    // async workers and, above all, off the UI thread.
    let content_type = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone());
    let (body, body_text, body_lines, json) = tokio::task::spawn_blocking(move || {
        let body_text = decode_preview(&body, truncated);
        let json = match &body_text {
            Some(text) if json::looks_like_json(content_type.as_deref(), text) => {
                if truncated {
                    JsonView::Truncated
                } else {
                    match json::validate(text) {
                        Ok(()) => JsonView::Pretty(TextLines::new(json::pretty(text))),
                        Err(e) => JsonView::Invalid(e),
                    }
                }
            }
            _ => JsonView::NotJson,
        };
        let body_lines = TextLines::new(body_text.clone().unwrap_or_default());
        (body, body_text, body_lines, json)
    })
    .await
    .map_err(|e| HttpError::Other(format!("response processing failed: {e}")))?;

    Ok(HttpResponse {
        status: status.as_u16(),
        reason: status.canonical_reason(),
        version,
        final_url,
        headers,
        body,
        body_text,
        body_lines,
        json,
        truncated,
        bytes_received,
        elapsed,
        saved_to,
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
