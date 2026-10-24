//! Stopping an in-flight send: `POST /collections/{id}/send/{send_id}/cancel`.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{PORT, call, write};
use serde_json::{Value, json};
use tower::ServiceExt;
use wiremock::matchers::{header, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SLOW: Duration = Duration::from_secs(5);

async fn mock() -> MockServer {
    let mock = MockServer::start().await;
    Mock::given(path("/slow"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("late")
                .set_delay(SLOW),
        )
        .mount(&mock)
        .await;
    Mock::given(path("/slow2"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("ok")
                .set_delay(Duration::from_millis(400)),
        )
        .mount(&mock)
        .await;
    Mock::given(path("/with-cookie"))
        .and(header("cookie", "sid=abc12345"))
        .respond_with(ResponseTemplate::new(200).set_body_string("cookie"))
        .mount(&mock)
        .await;
    Mock::given(path("/fast"))
        .respond_with(ResponseTemplate::new(200).set_body_string("fast"))
        .mount(&mock)
        .await;
    mock
}

fn request(root: &Path, name: &str, url: &str, extra: &str) {
    write(
        root,
        &format!("{name}.bru"),
        &format!(
            "meta {{\n  name: {name}\n  type: http\n  seq: 1\n}}\n\nget {{\n  url: {url}\n}}\n{extra}"
        ),
    );
}

/// A collection whose requests point at `base`.
fn collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"cancel","type":"collection"}"#,
    );
    request(root, "slow", &format!("{base}/slow"), "");
    request(root, "fast", &format!("{base}/fast"), "");
    request(
        root,
        "loop",
        &format!("{base}/fast"),
        "\nscript:pre-request {\n  while (true) {}\n}\n",
    );
    request(
        root,
        "setx",
        &format!("{base}/slow"),
        "\nscript:pre-request {\n  bru.setVar(\"x\", 1);\n}\n",
    );
    request(
        root,
        "setx2",
        &format!("{base}/slow2"),
        "\nscript:pre-request {\n  bru.setVar(\"y\", 2);\n}\n",
    );
    request(
        root,
        "checkx",
        &format!("{base}/fast"),
        "\ntests {\n  test(\"x unset\", () => expect(bru.getVar(\"x\")).to.equal(undefined));\n}\n",
    );
    request(
        root,
        "checky",
        &format!("{base}/fast"),
        "\ntests {\n  test(\"y kept\", () => expect(bru.getVar(\"y\")).to.equal(2));\n}\n",
    );
    request(
        root,
        "cookie",
        &format!("{base}/slow"),
        &format!(
            "\nscript:pre-request {{\n  await bru.cookies.jar().setCookie(\"{base}/\", \"sid\", \"abc12345\");\n}}\n"
        ),
    );
    request(root, "withcookie", &format!("{base}/with-cookie"), "");
    dir
}

async fn open(app: &axum::Router, root: &Path) -> String {
    let (status, body) = call(
        app,
        "POST",
        "/api/collections",
        Some(json!({ "path": root })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["id"].as_str().unwrap().to_owned()
}

fn send(
    app: &axum::Router,
    id: &str,
    item: &str,
    send_id: Option<&str>,
) -> tokio::task::JoinHandle<(StatusCode, Value)> {
    let app = app.clone();
    let uri = format!("/api/collections/{id}/send");
    let body = match send_id {
        Some(s) => json!({ "item": item, "sendId": s }),
        None => json!({ "item": item }),
    };
    tokio::spawn(async move { call(&app, "POST", &uri, Some(body)).await })
}

async fn cancel(app: &axum::Router, id: &str, send_id: &str) -> StatusCode {
    call(
        app,
        "POST",
        &format!("/api/collections/{id}/send/{send_id}/cancel"),
        None,
    )
    .await
    .0
}

/// Cancels `send_id` and returns the send's answer and how long it took after the cancel.
async fn cancel_and_wait(
    app: &axum::Router,
    id: &str,
    send_id: &str,
    sent: tokio::task::JoinHandle<(StatusCode, Value)>,
) -> ((StatusCode, Value), Duration) {
    tokio::time::sleep(Duration::from_millis(100)).await;
    let at = Instant::now();
    assert_eq!(cancel(app, id, send_id).await, StatusCode::NO_CONTENT);
    let answer = sent.await.unwrap();
    (answer, at.elapsed())
}

fn assert_cancelled((status, body): &(StatusCode, Value)) {
    assert_eq!(*status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "cancelled", "{body}");
}

#[tokio::test]
async fn cancel_stops_slow_send_quickly() {
    let mock = mock().await;
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let sent = send(&app, &id, "slow.bru", Some("s1"));
    let (answer, took) = cancel_and_wait(&app, &id, "s1", sent).await;
    assert_cancelled(&answer);
    assert!(took < Duration::from_millis(200), "took {took:?}");
}

#[tokio::test]
async fn cancel_interrupts_looping_pre_script() {
    let mock = mock().await;
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let sent = send(&app, &id, "loop.bru", Some("loop-1"));
    let (answer, took) = cancel_and_wait(&app, &id, "loop-1", sent).await;
    assert_cancelled(&answer);
    assert!(took < Duration::from_millis(200), "took {took:?}");
}

#[tokio::test]
async fn cancelled_send_keeps_no_runtime_vars() {
    let mock = mock().await;
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let sent = send(&app, &id, "setx.bru", Some("s1"));
    let (answer, _) = cancel_and_wait(&app, &id, "s1", sent).await;
    assert_cancelled(&answer);
    let (status, body) = send(&app, &id, "checkx.bru", None).await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["tests"][0]["passed"], true, "{body}");
}

#[tokio::test]
async fn cookies_before_cancel_are_kept() {
    let mock = mock().await;
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let sent = send(&app, &id, "cookie.bru", Some("c1"));
    let (answer, _) = cancel_and_wait(&app, &id, "c1", sent).await;
    assert_cancelled(&answer);
    let (status, body) = send(&app, &id, "withcookie.bru", None).await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["response"]["status"], 200, "{body}");
    assert_eq!(body["response"]["bodyText"], "cookie", "{body}");
}

#[tokio::test]
async fn cancel_unknown_is_204() {
    let mock = mock().await;
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    assert_eq!(cancel(&app, &id, "nobody").await, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn cancel_after_finish_is_noop_and_result_kept() {
    let mock = mock().await;
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (status, body) = send(&app, &id, "fast.bru", Some("f1")).await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["response"]["bodyText"], "fast");
    assert_eq!(cancel(&app, &id, "f1").await, StatusCode::NO_CONTENT);
    // The id is free again once the send ended.
    let (status, _) = send(&app, &id, "fast.bru", Some("f1")).await.unwrap();
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn duplicate_send_id_is_409() {
    let mock = mock().await;
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let first = send(&app, &id, "slow.bru", Some("dup"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    let (status, body) = send(&app, &id, "fast.bru", Some("dup")).await.unwrap();
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "duplicate_send");
    assert_eq!(cancel(&app, &id, "dup").await, StatusCode::NO_CONTENT);
    assert_cancelled(&first.await.unwrap());
}

#[tokio::test]
async fn cancel_one_of_two_concurrent_sends() {
    let mock = mock().await;
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let one = send(&app, &id, "setx.bru", Some("s1"));
    let two = send(&app, &id, "setx2.bru", Some("s2"));
    let (answer, _) = cancel_and_wait(&app, &id, "s1", one).await;
    assert_cancelled(&answer);
    let (status, body) = two.await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["response"]["status"], 200);
    let (_, body) = send(&app, &id, "checky.bru", None).await.unwrap();
    assert_eq!(body["tests"][0]["passed"], true, "{body}");
    let (_, body) = send(&app, &id, "checkx.bru", None).await.unwrap();
    assert_eq!(body["tests"][0]["passed"], true, "{body}");
}

#[tokio::test]
async fn send_without_send_id_still_works() {
    let mock = mock().await;
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (status, body) = send(&app, &id, "fast.bru", None).await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["response"]["bodyText"], "fast");
}

#[tokio::test]
async fn cancel_route_needs_token() {
    let mock = mock().await;
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/collections/{id}/send/s1/cancel"))
        .header("host", format!("127.0.0.1:{PORT}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn bad_send_id_is_400() {
    let mock = mock().await;
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let long = "a".repeat(65);
    for bad in ["a_b", long.as_str(), "a%20b"] {
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/send/{bad}/cancel"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {body}");
        assert_eq!(body["error"], "bad_send_id");
    }
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(json!({ "item": "fast.bru", "sendId": "a b" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "bad_send_id");
}
