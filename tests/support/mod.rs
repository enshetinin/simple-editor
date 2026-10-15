//! Deterministic local HTTP/1.1 (and HTTPS) test server. No external services.
//!
//! Endpoints:
//! - `/echo`              200, body = "METHOD\ncontent-type\nBODY"
//! - `/echo-all`          200, body = raw request line + headers + "\n\n" + body
//! - `/json`              200, `{"ok":true,"n":1}`
//! - `/json-invalid`      200 application/json with an invalid body
//! - `/empty`             204, no body
//! - `/status/500`        500 with a text body
//! - `/delay`             waits 10 s before answering; records early disconnects
//! - `/big`               6 MiB of 'x' (above the 5 MiB preview limit)
//! - `/gzip-big`          6 MiB of 'x', gzip-encoded (small on the wire)
//! - `/redirect`          302 to `/json`
//! - `/redirect-to?<url>` 302 to the absolute URL given as the whole query
//! - `/see-other`         303 to `/echo-all`
//! - `/temporary`         307 to `/echo-all`
//! - `/binary`            bytes containing NUL
//! - `/drop`              closes the connection after sending headers only

#![allow(dead_code)]

use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct TestServer {
    pub addr: SocketAddr,
    pub https: bool,
    /// Number of `/delay` connections closed by the client before the reply.
    pub delay_disconnects: Arc<AtomicUsize>,
    /// PEM of the self-signed certificate (HTTPS servers only).
    pub cert_pem: Option<String>,
    _runtime: tokio::runtime::Runtime,
}

impl TestServer {
    pub fn start() -> Self {
        Self::start_inner(false)
    }

    /// HTTPS with a freshly generated self-signed certificate for
    /// `localhost`. Clients must trust `cert_pem` explicitly.
    pub fn start_https() -> Self {
        Self::start_inner(true)
    }

    fn start_inner(https: bool) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let listener = runtime.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let addr = listener.local_addr().unwrap();
        let delay_disconnects = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&delay_disconnects);
        let (acceptor, cert_pem) = if https {
            let ck = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
            let key =
                rustls::pki_types::PrivateKeyDer::Pkcs8(ck.signing_key.serialize_der().into());
            let config = rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![ck.cert.der().clone()], key)
                .unwrap();
            (
                Some(tokio_rustls::TlsAcceptor::from(Arc::new(config))),
                Some(ck.cert.pem()),
            )
        } else {
            (None, None)
        };
        runtime.spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let counter = Arc::clone(&counter);
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    match acceptor {
                        Some(a) => {
                            if let Ok(tls) = a.accept(stream).await {
                                let _ = handle(tls, counter).await;
                            }
                        }
                        None => {
                            let _ = handle(stream, counter).await;
                        }
                    }
                });
            }
        });
        Self {
            addr,
            https,
            delay_disconnects,
            cert_pem,
            _runtime: runtime,
        }
    }

    pub fn url(&self, path: &str) -> String {
        if self.https {
            format!("https://localhost:{}{}", self.addr.port(), path)
        } else {
            format!("http://{}{}", self.addr, path)
        }
    }
}

struct Request {
    method: String,
    path: String,
    content_type: String,
    head: String,
    body: Vec<u8>,
}

async fn read_request<S: AsyncRead + Unpin>(stream: &mut S) -> std::io::Result<Option<Request>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let method = first.next().unwrap_or("").to_owned();
    let path = first.next().unwrap_or("").to_owned();
    let mut content_length = 0usize;
    let mut content_type = String::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => content_length = v.trim().parse().unwrap_or(0),
                "content-type" => content_type = v.trim().to_owned(),
                _ => {}
            }
        }
    }
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(Some(Request {
        method,
        path,
        content_type,
        head,
        body,
    }))
}

async fn respond<S: AsyncWrite + Unpin>(
    stream: &mut S,
    status: &str,
    extra_headers: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n{extra_headers}\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.shutdown().await
}

async fn handle<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    delay_disconnects: Arc<AtomicUsize>,
) -> std::io::Result<()> {
    let Some(req) = read_request(&mut stream).await? else {
        return Ok(());
    };
    let (path, query) = match req.path.split_once('?') {
        Some((p, q)) => (p.to_owned(), q.to_owned()),
        None => (req.path.clone(), String::new()),
    };
    match path.as_str() {
        "/echo" => {
            let mut body = format!("{}\n{}\n", req.method, req.content_type).into_bytes();
            body.extend_from_slice(&req.body);
            respond(
                &mut stream,
                "200 OK",
                "",
                "text/plain; charset=utf-8",
                &body,
            )
            .await
        }
        "/echo-all" => {
            let mut body = req.head.replace("\r\n", "\n").into_bytes();
            body.extend_from_slice(b"\n\n");
            body.extend_from_slice(&req.body);
            respond(
                &mut stream,
                "200 OK",
                "",
                "text/plain; charset=utf-8",
                &body,
            )
            .await
        }
        "/json" => {
            respond(
                &mut stream,
                "200 OK",
                "",
                "application/json",
                br#"{"ok":true,"n":1}"#,
            )
            .await
        }
        "/json-invalid" => {
            respond(
                &mut stream,
                "200 OK",
                "",
                "application/json",
                b"{\"a\": tru",
            )
            .await
        }
        "/empty" => {
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n")
                .await?;
            stream.shutdown().await
        }
        "/status/500" => {
            respond(
                &mut stream,
                "500 Internal Server Error",
                "",
                "text/plain",
                b"boom",
            )
            .await
        }
        "/delay" => {
            let mut probe = [0u8; 1];
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(10)) => {
                    respond(&mut stream, "200 OK", "", "text/plain", b"late").await
                }
                read = stream.read(&mut probe) => {
                    if matches!(read, Ok(0) | Err(_)) {
                        delay_disconnects.fetch_add(1, Ordering::SeqCst);
                    }
                    Ok(())
                }
            }
        }
        "/big" => {
            let body = vec![b'x'; 6 * 1024 * 1024];
            respond(&mut stream, "200 OK", "", "text/plain", &body).await
        }
        "/gzip-big" => {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
            enc.write_all(&vec![b'x'; 6 * 1024 * 1024]).unwrap();
            let gz = enc.finish().unwrap();
            respond(
                &mut stream,
                "200 OK",
                "content-encoding: gzip\r\n",
                "text/plain",
                &gz,
            )
            .await
        }
        "/redirect" => {
            respond(
                &mut stream,
                "302 Found",
                "location: /json\r\n",
                "text/plain",
                b"",
            )
            .await
        }
        "/redirect-to" => {
            let location = format!("location: {query}\r\n");
            respond(&mut stream, "302 Found", &location, "text/plain", b"").await
        }
        "/see-other" => {
            respond(
                &mut stream,
                "303 See Other",
                "location: /echo-all\r\n",
                "text/plain",
                b"",
            )
            .await
        }
        "/temporary" => {
            respond(
                &mut stream,
                "307 Temporary Redirect",
                "location: /echo-all\r\n",
                "text/plain",
                b"",
            )
            .await
        }
        "/binary" => {
            respond(
                &mut stream,
                "200 OK",
                "",
                "application/octet-stream",
                &[0x89, b'P', b'N', b'G', 0, 1, 2, 3],
            )
            .await
        }
        "/drop" => {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 1000\r\n\r\npartial")
                .await?;
            // Close without sending the promised body.
            Ok(())
        }
        _ => respond(&mut stream, "404 Not Found", "", "text/plain", b"not found").await,
    }
}
