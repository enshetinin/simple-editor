mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use simpleeditor::http::client::DEFAULT_PREVIEW_LIMIT;
use simpleeditor::http::{
    BodyKind, ClientConfig, HttpError, HttpResponse, HttpService, Method, RequestDraft,
};
use support::TestServer;

const WAIT: Duration = Duration::from_secs(10);

fn service(config: ClientConfig) -> HttpService {
    HttpService::new(config, Arc::new(|| {})).expect("http service starts")
}

fn draft(method: Method, url: String) -> RequestDraft {
    RequestDraft {
        method,
        url,
        ..RequestDraft::default()
    }
}

fn send(svc: &mut HttpService, d: RequestDraft) -> Result<HttpResponse, HttpError> {
    let id = svc.send(d.prepare().expect("valid request"));
    let event = svc.wait(WAIT).expect("event arrives");
    assert_eq!(event.id, id);
    event.result
}

#[test]
fn post_json_sends_method_content_type_and_exact_body() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let body = r#"{"name":"café 👋"}"#;
    let mut d = draft(Method::Post, server.url("/echo"));
    d.body = body.to_owned();
    d.body_kind = BodyKind::Json;
    let r = send(&mut svc, d).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(
        r.body_text.as_deref(),
        Some(format!("POST\napplication/json\n{body}").as_str())
    );
    assert!(!r.truncated);
    assert_eq!(r.bytes_received, r.body.len() as u64);
}

#[test]
fn all_methods_reach_the_server() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    for m in Method::ALL {
        let r = send(&mut svc, draft(m, server.url("/echo"))).unwrap();
        assert_eq!(r.status, 200, "{m:?}");
        if m != Method::Head {
            assert!(r.body_text.unwrap().starts_with(m.as_str()), "{m:?}");
        } else {
            assert!(r.body.is_empty());
        }
    }
}

#[test]
fn server_errors_are_responses_not_transport_errors() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let r = send(&mut svc, draft(Method::Get, server.url("/status/500"))).unwrap();
    assert_eq!(r.status, 500);
    assert_eq!(r.body_text.as_deref(), Some("boom"));

    let r = send(&mut svc, draft(Method::Get, server.url("/empty"))).unwrap();
    assert_eq!(r.status, 204);
    assert!(r.body.is_empty());

    let r = send(&mut svc, draft(Method::Get, server.url("/binary"))).unwrap();
    assert!(r.body_text.is_none(), "binary body is not shown as text");
}

#[test]
fn redirect_is_followed_and_final_url_reported() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let r = send(&mut svc, draft(Method::Get, server.url("/redirect"))).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.final_url, server.url("/json"));
}

#[test]
fn large_body_is_truncated_at_preview_limit() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let r = send(&mut svc, draft(Method::Get, server.url("/big"))).unwrap();
    assert!(r.truncated);
    assert_eq!(r.body.len(), DEFAULT_PREVIEW_LIMIT);
    assert!(r.bytes_received >= DEFAULT_PREVIEW_LIMIT as u64);
    assert!(r.bytes_received < 6 * 1024 * 1024 + 1);
}

#[test]
fn cancel_drops_the_connection_and_delivers_no_result() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let id = svc.send(draft(Method::Get, server.url("/delay")).prepare().unwrap());
    std::thread::sleep(Duration::from_millis(300));
    let cancelled_at = Instant::now();
    svc.cancel(id);
    assert_eq!(svc.in_flight(), 0);

    // The server sees the client disconnect promptly.
    while server.delay_disconnects.load(Ordering::SeqCst) == 0 {
        assert!(
            cancelled_at.elapsed() < Duration::from_secs(3),
            "connection still open after cancel"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(svc.wait(Duration::from_millis(300)).is_none());
    assert!(svc.poll().is_empty());

    // The service stays usable.
    let r = send(&mut svc, draft(Method::Get, server.url("/json"))).unwrap();
    assert_eq!(r.status, 200);
}

#[test]
fn timeout_is_reported_as_timeout() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig {
        timeout: Duration::from_millis(500),
        ..ClientConfig::default()
    });
    let started = Instant::now();
    let err = send(&mut svc, draft(Method::Get, server.url("/delay"))).unwrap_err();
    assert_eq!(err, HttpError::Timeout);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn connection_refused_and_broken_body_are_distinguished() {
    // Bind and drop a listener to get a local port with nothing listening.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut svc = service(ClientConfig::default());
    let err = send(
        &mut svc,
        draft(Method::Get, format!("http://127.0.0.1:{port}/")),
    )
    .unwrap_err();
    assert!(matches!(err, HttpError::Connect(_)), "{err:?}");

    let server = TestServer::start();
    let err = send(&mut svc, draft(Method::Get, server.url("/drop"))).unwrap_err();
    assert!(matches!(err, HttpError::Body(_)), "{err:?}");
}

#[test]
fn unknown_host_is_a_dns_error() {
    let mut svc = service(ClientConfig::default());
    // `.invalid` is reserved (RFC 6761) and never resolves.
    let err = send(
        &mut svc,
        draft(Method::Get, "http://simpleeditor-test.invalid/".to_owned()),
    )
    .unwrap_err();
    assert!(matches!(err, HttpError::Dns(_)), "{err:?}");
}

#[test]
fn newer_request_result_is_not_confused_with_cancelled_one() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let slow = svc.send(draft(Method::Get, server.url("/delay")).prepare().unwrap());
    svc.cancel(slow);
    let fast = svc.send(draft(Method::Get, server.url("/json")).prepare().unwrap());
    let event = svc.wait(WAIT).unwrap();
    assert_eq!(event.id, fast);
    assert_ne!(event.id, slow);
}
