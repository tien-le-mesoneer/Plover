mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{PORT, app};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn get(path: &str) -> (StatusCode, axum::http::HeaderMap, String) {
    let req = Request::get(path)
        .header("host", format!("127.0.0.1:{PORT}"))
        .body(Body::empty())
        .unwrap();
    let res = app().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        headers,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

#[tokio::test]
async fn index_served_without_token() {
    let (status, headers, body) = get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert_eq!(headers["cache-control"], "no-cache");
    assert!(
        body.contains("<html") || body.contains("<!doctype html"),
        "{body}"
    );
}

#[tokio::test]
async fn spa_fallback() {
    let (_, _, index) = get("/").await;
    let (status, headers, body) = get("/collections/abc/requests").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert_eq!(body, index);
}

#[tokio::test]
async fn missing_asset_file_is_404_not_index() {
    let (status, _, _) = get("/assets/does-not-exist-1234.js").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn api_not_shadowed_by_fallback() {
    let (status, headers, body) = get("/api/x").await;
    // No token: the API layer answers, not the SPA fallback.
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    assert!(body.contains("unauthorized"));
}

#[tokio::test]
async fn csp_header_present() {
    let (_, headers, _) = get("/").await;
    let csp = headers["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("connect-src 'self' ws://127.0.0.1:* ws://localhost:*"));
    assert!(csp.contains("worker-src 'self' blob:"));
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["referrer-policy"], "no-referrer");
}

#[tokio::test]
async fn static_files_still_check_host() {
    let req = Request::get("/")
        .header("host", "evil.example")
        .body(Body::empty())
        .unwrap();
    let res = app().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}
