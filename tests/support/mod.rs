//! Deterministic local HTTP/1.1 test server (no external services).
//!
//! Endpoints:
//! - `/echo`            200, body = "METHOD\ncontent-type\nBODY"
//! - `/json`            200, `{"ok":true,"n":1}`
//! - `/empty`           204, no body
//! - `/status/500`      500 with a text body
//! - `/delay`           waits 10 s before answering; records early disconnects
//! - `/big`             6 MiB of 'x' (above the 5 MiB preview limit)
//! - `/redirect`        302 to `/json`
//! - `/binary`          bytes containing NUL
//! - `/drop`            closes the connection after sending headers only

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub struct TestServer {
    pub addr: SocketAddr,
    /// Number of `/delay` connections closed by the client before the reply.
    pub delay_disconnects: Arc<AtomicUsize>,
    _runtime: tokio::runtime::Runtime,
}

impl TestServer {
    pub fn start() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let listener = runtime.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let addr = listener.local_addr().unwrap();
        let delay_disconnects = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&delay_disconnects);
        runtime.spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let counter = Arc::clone(&counter);
                tokio::spawn(async move {
                    let _ = handle(stream, counter).await;
                });
            }
        });
        Self {
            addr,
            delay_disconnects,
            _runtime: runtime,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

struct Request {
    method: String,
    path: String,
    content_type: String,
    body: Vec<u8>,
}

async fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
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
        body,
    }))
}

async fn respond(
    stream: &mut TcpStream,
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

async fn handle(mut stream: TcpStream, delay_disconnects: Arc<AtomicUsize>) -> std::io::Result<()> {
    let Some(req) = read_request(&mut stream).await? else {
        return Ok(());
    };
    match req.path.as_str() {
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
