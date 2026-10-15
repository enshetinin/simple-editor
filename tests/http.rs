mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use simpleeditor::http::client::DEFAULT_PREVIEW_LIMIT;
use simpleeditor::http::{
    Auth, Body, BodyMode, BodySink, ClientConfig, HttpError, HttpResponse, HttpService, JsonView,
    KeyValue, Method, RequestDefinition, ResolvedRequest, Variable, resolve,
};
use support::TestServer;

const WAIT: Duration = Duration::from_secs(15);

fn service(config: ClientConfig) -> HttpService {
    HttpService::new(config, Arc::new(|| {})).expect("http service starts")
}

fn def(method: Method, url: String) -> RequestDefinition {
    RequestDefinition {
        method,
        url,
        ..RequestDefinition::default()
    }
}

fn resolved(d: &RequestDefinition) -> ResolvedRequest {
    resolve(d, &[], &[]).expect("valid request")
}

fn send_to(
    svc: &mut HttpService,
    r: ResolvedRequest,
    sink: BodySink,
) -> Result<HttpResponse, HttpError> {
    let id = svc.send_to(r, sink);
    let event = svc.wait(WAIT).expect("event arrives");
    assert_eq!(event.id, id);
    event.result
}

fn send(svc: &mut HttpService, d: RequestDefinition) -> Result<HttpResponse, HttpError> {
    send_to(svc, resolved(&d), BodySink::Preview)
}

fn text(r: &HttpResponse) -> &str {
    r.body_text.as_deref().unwrap_or("")
}

#[test]
fn post_json_sends_method_content_type_and_exact_body() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let body = r#"{"name":"café 👋"}"#;
    let mut d = def(Method::Post, server.url("/echo"));
    d.body = Body {
        mode: BodyMode::Json,
        text: body.to_owned(),
    };
    let r = send(&mut svc, d).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(text(&r), format!("POST\napplication/json\n{body}"));
    assert!(!r.truncated);
}

#[test]
fn repeated_query_params_headers_and_variables_reach_the_server() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let d = RequestDefinition {
        url: "{{base}}/echo-all?fixed=1".into(),
        params: vec![
            KeyValue::new("tag", "a b"),
            KeyValue::new("tag", "ñ"),
            KeyValue {
                enabled: false,
                ..KeyValue::new("off", "x")
            },
        ],
        headers: vec![
            KeyValue::new("X-Multi", "1"),
            KeyValue::new("X-Multi", "{{two}}"),
        ],
        auth: Auth::Bearer {
            token: "{{token}}".into(),
        },
        variables: vec![Variable::plain("two", "2-from-request")],
        ..RequestDefinition::default()
    };
    let env = [
        Variable::plain("base", server.url("")),
        Variable::plain("two", "2-from-env"),
        Variable::secret("token", "s3cr3t"),
    ];
    let r = resolve(&d, &env, &[]).unwrap();
    let r = send_to(&mut svc, r, BodySink::Preview).unwrap();
    let echo = text(&r).to_ascii_lowercase();
    assert!(
        echo.starts_with("get /echo-all?fixed=1&tag=a+b&tag=%c3%b1 "),
        "{echo}"
    );
    assert!(
        echo.contains("x-multi: 1\nx-multi: 2-from-request\n"),
        "{echo}"
    );
    assert!(echo.contains("authorization: bearer s3cr3t"), "{echo}");
    assert!(!echo.contains("off=x"));
    assert!(
        echo.contains("accept-encoding: "),
        "decompression is negotiated"
    );
}

#[test]
fn basic_auth_header_is_exact() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let mut d = def(Method::Get, server.url("/echo-all"));
    d.auth = Auth::Basic {
        username: "ana".into(),
        password: "pässwörd".into(),
    };
    let r = send(&mut svc, d).unwrap();
    // base64("ana:pässwörd")
    assert!(
        text(&r).contains("authorization: Basic YW5hOnDDpHNzd8O2cmQ="),
        "{}",
        text(&r)
    );
}

#[test]
fn all_methods_reach_the_server() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    for m in Method::ALL {
        let r = send(&mut svc, def(m, server.url("/echo"))).unwrap();
        assert_eq!(r.status, 200, "{m:?}");
        if m != Method::Head {
            assert!(text(&r).starts_with(m.as_str()), "{m:?}");
        } else {
            assert!(r.body.is_empty());
        }
    }
}

#[test]
fn server_errors_empty_binary_and_json_views() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let r = send(&mut svc, def(Method::Get, server.url("/status/500"))).unwrap();
    assert_eq!(r.status, 500);
    assert_eq!(text(&r), "boom");

    let r = send(&mut svc, def(Method::Get, server.url("/empty"))).unwrap();
    assert_eq!(r.status, 204);
    assert!(r.body.is_empty());

    let r = send(&mut svc, def(Method::Get, server.url("/binary"))).unwrap();
    assert!(r.body_text.is_none(), "binary body is not shown as text");

    let r = send(&mut svc, def(Method::Get, server.url("/json"))).unwrap();
    match &r.json {
        JsonView::Pretty(p) => {
            assert_eq!(p.text, "{\n  \"ok\": true,\n  \"n\": 1\n}");
            assert_eq!(p.lines.len(), 4);
        }
        other => panic!("{other:?}"),
    }
    let r = send(&mut svc, def(Method::Get, server.url("/json-invalid"))).unwrap();
    assert!(matches!(r.json, JsonView::Invalid(_)), "{:?}", r.json);
    assert_eq!(text(&r), "{\"a\": tru", "raw body still shown");
}

#[test]
fn redirects_report_final_url_and_follow_method_rules() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let r = send(&mut svc, def(Method::Get, server.url("/redirect"))).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.final_url, server.url("/json"));

    let post = |path: &str| {
        let mut d = def(Method::Post, server.url(path));
        d.body = Body {
            mode: BodyMode::Text,
            text: "payload".into(),
        };
        d
    };
    // 303: becomes GET without body.
    let r = send(&mut svc, post("/see-other")).unwrap();
    assert!(text(&r).starts_with("GET /echo-all"), "{}", text(&r));
    assert!(!text(&r).contains("payload"));
    // 307: method and body are kept.
    let r = send(&mut svc, post("/temporary")).unwrap();
    assert!(text(&r).starts_with("POST /echo-all"), "{}", text(&r));
    assert!(text(&r).ends_with("payload"));
}

#[test]
fn authorization_is_not_forwarded_to_another_origin() {
    let a = TestServer::start();
    let b = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let with_auth = |url: String| {
        let mut d = def(Method::Get, url);
        d.auth = Auth::Bearer {
            token: "top-secret".into(),
        };
        d.headers = vec![KeyValue::new("Cookie", "sid=1")];
        d
    };
    // Same origin: kept.
    let same = format!("{}?{}", a.url("/redirect-to"), a.url("/echo-all"));
    let r = send(&mut svc, with_auth(same)).unwrap();
    assert!(text(&r).contains("top-secret"));
    // Different origin (other port): removed.
    let other = format!("{}?{}", a.url("/redirect-to"), b.url("/echo-all"));
    let r = send(&mut svc, with_auth(other)).unwrap();
    assert_eq!(r.final_url, b.url("/echo-all"));
    let echo = text(&r).to_ascii_lowercase();
    assert!(
        !echo.contains("authorization") && !echo.contains("top-secret"),
        "{echo}"
    );
    assert!(!echo.contains("cookie"), "{echo}");
}

#[test]
fn more_than_ten_redirects_is_an_error() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let mut url = server.url("/json");
    for _ in 0..11 {
        url = format!("{}?{url}", server.url("/redirect-to"));
    }
    let err = send(&mut svc, def(Method::Get, url)).unwrap_err();
    assert!(matches!(err, HttpError::Redirect(_)), "{err:?}");

    let mut url = server.url("/json");
    for _ in 0..10 {
        url = format!("{}?{url}", server.url("/redirect-to"));
    }
    assert_eq!(send(&mut svc, def(Method::Get, url)).unwrap().status, 200);
}

#[test]
fn preview_limit_applies_to_decompressed_bytes() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    for path in ["/big", "/gzip-big"] {
        let r = send(&mut svc, def(Method::Get, server.url(path))).unwrap();
        assert!(r.truncated, "{path}");
        assert_eq!(r.body.len(), DEFAULT_PREVIEW_LIMIT, "{path}");
        assert!(r.body.iter().all(|&b| b == b'x'), "{path}: decompressed");
        assert!(r.bytes_received >= DEFAULT_PREVIEW_LIMIT as u64);
        assert!(r.bytes_received <= 6 * 1024 * 1024, "{path}");
    }
}

#[test]
fn download_to_file_respects_limit_and_never_leaves_partial_files() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("big.bin");

    let r = send_to(
        &mut svc,
        resolved(&def(Method::Get, server.url("/big"))),
        BodySink::File {
            path: target.clone(),
            max_bytes: 10 * 1024 * 1024,
        },
    )
    .unwrap();
    assert_eq!(r.saved_to.as_deref(), Some(target.as_path()));
    assert_eq!(std::fs::metadata(&target).unwrap().len(), 6 * 1024 * 1024);
    assert!(r.body.is_empty(), "nothing kept in memory");
    std::fs::remove_file(&target).unwrap();

    let err = send_to(
        &mut svc,
        resolved(&def(Method::Get, server.url("/gzip-big"))),
        BodySink::File {
            path: target.clone(),
            max_bytes: 1024 * 1024,
        },
    )
    .unwrap_err();
    assert_eq!(err, HttpError::TooLarge(1024 * 1024));
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        0,
        "no partial file"
    );

    // Cancel during a download: nothing written either.
    let id = svc.send_to(
        resolved(&def(Method::Get, server.url("/delay"))),
        BodySink::File {
            path: target.clone(),
            max_bytes: 1024,
        },
    );
    std::thread::sleep(Duration::from_millis(200));
    svc.cancel(id);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn tls_is_verified_and_a_test_certificate_can_be_trusted_explicitly() {
    let server = TestServer::start_https();
    let mut svc = service(ClientConfig::default());
    let err = send(&mut svc, def(Method::Get, server.url("/json"))).unwrap_err();
    assert!(matches!(err, HttpError::Tls(_)), "{err:?}");

    let trusted = ClientConfig {
        extra_roots_pem: vec![server.cert_pem.clone().unwrap().into_bytes()],
        ..ClientConfig::default()
    };
    let mut svc = service(trusted);
    let r = send(&mut svc, def(Method::Get, server.url("/json"))).unwrap();
    assert_eq!(r.status, 200);
    assert!(r.final_url.starts_with("https://"));
}

#[test]
fn cancel_drops_the_connection_and_delivers_no_result() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let id = svc.send(resolved(&def(Method::Get, server.url("/delay"))));
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
    let r = send(&mut svc, def(Method::Get, server.url("/json"))).unwrap();
    assert_eq!(r.status, 200);
}

#[test]
fn per_request_timeout_is_reported_as_timeout() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let mut d = def(Method::Get, server.url("/delay"));
    d.timeout_secs = 1;
    let started = Instant::now();
    let err = send(&mut svc, d).unwrap_err();
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
        def(Method::Get, format!("http://127.0.0.1:{port}/")),
    )
    .unwrap_err();
    assert!(matches!(err, HttpError::Connect(_)), "{err:?}");

    let server = TestServer::start();
    let err = send(&mut svc, def(Method::Get, server.url("/drop"))).unwrap_err();
    assert!(matches!(err, HttpError::Body(_)), "{err:?}");
}

#[test]
fn unknown_host_is_a_dns_error() {
    let mut svc = service(ClientConfig::default());
    // `.invalid` is reserved (RFC 6761) and never resolves.
    let err = send(
        &mut svc,
        def(Method::Get, "http://simpleeditor-test.invalid/".to_owned()),
    )
    .unwrap_err();
    assert!(matches!(err, HttpError::Dns(_)), "{err:?}");
}

#[test]
fn newer_request_result_is_not_confused_with_cancelled_one() {
    let server = TestServer::start();
    let mut svc = service(ClientConfig::default());
    let slow = svc.send(resolved(&def(Method::Get, server.url("/delay"))));
    svc.cancel(slow);
    let fast = svc.send(resolved(&def(Method::Get, server.url("/json"))));
    let event = svc.wait(WAIT).unwrap();
    assert_eq!(event.id, fast);
    assert_ne!(event.id, slow);
}
