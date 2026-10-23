use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use plover_core::executor::ExecOptions;
use plover_core::workspace::Workspace;
use plover_server::{AllowList, AppState, ServerConfig, router, start};
use tower::ServiceExt;

const PORT: u16 = 4321;
const TOKEN: &str = "test-token";

fn app(dev_origins: &[String]) -> axum::Router {
    router(AppState {
        workspace: Arc::new(Workspace::new(ExecOptions::default())),
        token: Arc::from(TOKEN),
        allowed: Arc::new(AllowList::new(PORT, dev_origins)),
        native: None,
        session_url: Arc::from(""),
    })
}

fn health() -> axum::http::request::Builder {
    Request::get("/api/health").header("host", format!("127.0.0.1:{PORT}"))
}

async fn status_and_body(app: axum::Router, req: Request<Body>) -> (StatusCode, String) {
    let res = app.oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn health_requires_token() {
    let (status, body) = status_and_body(app(&[]), health().body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains(r#""error":"unauthorized""#), "{body}");

    let wrong = health()
        .header("authorization", "Bearer nope")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        status_and_body(app(&[]), wrong).await.0,
        StatusCode::UNAUTHORIZED
    );

    let ok = health()
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, body) = status_and_body(app(&[]), ok).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"ok":true}"#);
}

#[tokio::test]
async fn rejects_foreign_origin() {
    let req = health()
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("origin", "https://evil.example")
        .body(Body::empty())
        .unwrap();
    let (status, body) = status_and_body(app(&[]), req).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("forbidden_origin"), "{body}");
}

#[tokio::test]
async fn rejects_rebound_host() {
    let req = Request::get("/api/health")
        .header("host", format!("evil.example:{PORT}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, body) = status_and_body(app(&[]), req).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("forbidden_host"), "{body}");

    // A different port on localhost is another origin too.
    let req = Request::get("/api/health")
        .header("host", "localhost:1")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        status_and_body(app(&[]), req).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn allows_missing_origin_and_dev_origin() {
    for (host, origin) in [
        (
            format!("localhost:{PORT}"),
            Some(format!("http://localhost:{PORT}")),
        ),
        (
            format!("127.0.0.1:{PORT}"),
            Some(format!("http://127.0.0.1:{PORT}")),
        ),
        (format!("127.0.0.1:{PORT}"), None),
        (
            "localhost:5173".to_owned(),
            Some("http://localhost:5173".to_owned()),
        ),
    ] {
        let mut req = Request::get("/api/health")
            .header("host", &host)
            .header("authorization", format!("Bearer {TOKEN}"));
        if let Some(origin) = &origin {
            req = req.header("origin", origin);
        }
        let app = app(&["http://localhost:5173".to_owned()]);
        let (status, body) = status_and_body(app, req.body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{host} {origin:?}: {body}");
    }
    // Without --dev-origin the Vite origin is foreign.
    let req = Request::get("/api/health")
        .header("host", "localhost:5173")
        .header("origin", "http://localhost:5173")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        status_and_body(app(&[]), req).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn start_binds_loopback_random_port() {
    let (running, server) = start(
        ServerConfig {
            port: 0,
            dev_origins: Vec::new(),
            exec: ExecOptions::default(),
            ..ServerConfig::default()
        },
        std::future::pending(),
    )
    .await
    .unwrap();
    assert!(running.addr.ip().is_loopback());
    assert_eq!(running.addr.ip().to_string(), "127.0.0.1");
    assert_ne!(running.addr.port(), 0);
    assert_eq!(running.token.len(), 64);
    assert!(running.token.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(
        running.url,
        format!(
            "http://127.0.0.1:{}/#token={}",
            running.addr.port(),
            running.token
        )
    );
    let handle = tokio::spawn(server);
    // A raw HTTP request over TCP proves the listener serves the router.
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(running.addr).await.unwrap();
    let req = format!(
        "GET /api/health HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        running.addr, running.token
    );
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    handle.abort();
}
