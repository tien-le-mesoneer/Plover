//! Launch codes (URLs the app opens carry a single-use code, never the token), the
//! server→UI event channel, and the UI's "client ready" signal.

mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{PORT, TOKEN, call};
use futures_util::StreamExt;
use http_body_util::BodyExt;
use plover_core::executor::ExecOptions;
use plover_server::{AllowList, AppState, ServerConfig, ServerEvent, SessionCodes, router, start};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower::ServiceExt;

fn state(codes: SessionCodes) -> AppState {
    AppState::new(ExecOptions::default(), TOKEN, AllowList::new(PORT, &[])).with_codes(codes)
}

/// POST /api/session/exchange without any token, as a freshly opened page would.
async fn exchange(app: &axum::Router, code: &str, origin: Option<&str>) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/session/exchange")
        .header("host", format!("127.0.0.1:{PORT}"))
        .header("content-type", "application/json");
    if let Some(origin) = origin {
        req = req.header("origin", origin);
    }
    let res = app
        .clone()
        .oneshot(
            req.body(Body::from(json!({ "code": code }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn launch_code_is_single_use() {
    let state = state(SessionCodes::new(Duration::from_secs(60)));
    let code = state.codes.mint();
    assert!(code.len() >= 32);
    let app = router(state);
    let (status, body) = exchange(&app, &code, Some(&format!("http://127.0.0.1:{PORT}"))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "token": TOKEN }));
    let (status, body) = exchange(&app, &code, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], json!("invalid_code"));
}

#[tokio::test]
async fn expired_and_wrong_codes_are_rejected() {
    let state = state(SessionCodes::new(Duration::from_millis(50)));
    let code = state.codes.mint();
    let app = router(state);
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(
        exchange(&app, &code, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        exchange(&app, "nope", None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn exchange_still_checks_origin() {
    let state = state(SessionCodes::new(Duration::from_secs(60)));
    let code = state.codes.mint();
    let app = router(state);
    let (status, _) = exchange(&app, &code, Some("https://evil.example")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // The refused attempt did not burn the code.
    assert_eq!(exchange(&app, &code, None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn repeated_wrong_codes_are_rate_limited() {
    let state = state(SessionCodes::new(Duration::from_secs(60)));
    let code = state.codes.mint();
    let app = router(state);
    let mut statuses = Vec::new();
    for i in 0..25 {
        statuses.push(exchange(&app, &format!("wrong-{i}"), None).await.0);
    }
    assert!(
        statuses.contains(&StatusCode::TOO_MANY_REQUESTS),
        "{statuses:?}"
    );
    // Once limited, even the right code waits for the window to pass.
    assert_eq!(
        exchange(&app, &code, None).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn client_ready_needs_the_token_and_is_observable() {
    let state = state(SessionCodes::new(Duration::from_secs(60)));
    let mut ready = state.client_ready.subscribe();
    let app = router(state);
    assert!(!*ready.borrow());
    let (status, _) = call(&app, "POST", "/api/client-ready", Some(json!({}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    tokio::time::timeout(Duration::from_secs(1), ready.wait_for(|r| *r))
        .await
        .unwrap()
        .unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/api/client-ready")
        .header("host", format!("127.0.0.1:{PORT}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn events_reach_subscribed_clients() {
    let (running, fut) = start(ServerConfig::default(), std::future::pending())
        .await
        .unwrap();
    tokio::spawn(fut);
    let url = format!("ws://{}/api/events", running.addr);
    // Without the token subprotocol the upgrade is refused.
    assert!(
        tokio_tungstenite::connect_async(url.as_str())
            .await
            .is_err()
    );
    let mut req = url.into_client_request().unwrap();
    req.headers_mut().insert(
        "sec-websocket-protocol",
        format!("plover.token.{}", running.token).parse().unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    // The subscription is registered once the upgrade completes.
    tokio::time::timeout(Duration::from_secs(2), async {
        while running.events.receivers() == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    running
        .events
        .send(ServerEvent::CollectionOpened { id: "c1".into() });
    running.events.send(ServerEvent::ReloadRequested);
    let mut got = Vec::new();
    for _ in 0..2 {
        let msg = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        got.push(serde_json::from_str::<Value>(msg.to_text().unwrap()).unwrap());
    }
    assert_eq!(
        got,
        [
            json!({ "type": "collection_opened", "id": "c1" }),
            json!({ "type": "reload_requested" })
        ]
    );
}

#[tokio::test]
async fn launch_url_carries_a_code_not_the_token() {
    let (running, fut) = start(ServerConfig::default(), std::future::pending())
        .await
        .unwrap();
    tokio::spawn(fut);
    let url = running.launch_url();
    assert!(
        url.starts_with(&format!("http://{}/#code=", running.addr)),
        "{url}"
    );
    assert!(!url.contains(&running.token));
    assert_ne!(running.launch_url(), url, "each URL gets a fresh code");
}
