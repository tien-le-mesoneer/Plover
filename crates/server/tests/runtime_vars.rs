//! `GET/DELETE /collections/{id}/runtime-vars`: list with masking, delete one, clear all.

mod common;

use std::path::Path;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{call, small_collection, write};
use futures_util::{SinkExt, StreamExt};
use plover_core::executor::ExecOptions;
use plover_server::{RunningServer, ServerConfig, start};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower::ServiceExt;
use wiremock::matchers::path_regex;
use wiremock::{Mock, MockServer, ResponseTemplate};

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

async fn set(app: &axum::Router, id: &str, name: &str, value: Value) {
    let (status, body) = call(
        app,
        "PUT",
        &format!("/api/collections/{id}/runtime-vars/{name}"),
        Some(json!({ "value": value })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

async fn list(app: &axum::Router, id: &str, env: Option<&str>) -> Value {
    let uri = match env {
        Some(env) => format!("/api/collections/{id}/runtime-vars?env={env}"),
        None => format!("/api/collections/{id}/runtime-vars"),
    };
    let (status, body) = call(app, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn names(dto: &Value) -> Vec<&str> {
    dto["vars"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect()
}

fn var<'a>(dto: &'a Value, name: &str) -> &'a Value {
    dto["vars"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap_or_else(|| panic!("{name} missing in {dto}"))
}

async fn mock() -> MockServer {
    let mock = MockServer::start().await;
    Mock::given(path_regex(".*"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock)
        .await;
    mock
}

#[tokio::test]
async fn list_masks_credential_name_sent_value_and_secret_value() {
    let mock = mock().await;
    let dir = small_collection(&mock.uri());
    write(
        dir.path(),
        "send.bru",
        "meta {\n  name: send\n  type: http\n  seq: 5\n}\n\nget {\n  url: {{host}}/send\n}\n\nheaders {\n  Authorization: Bearer {{docId}}\n}\n",
    );
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (status, _) = call(
        &app,
        "PUT",
        &format!("/api/collections/{id}/environments/local/secrets"),
        Some(json!({ "values": { "token": "s3cr3t-1" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    set(&app, &id, "token", json!("x1")).await;
    set(&app, &id, "docId", json!("abc12345")).await;
    set(&app, &id, "copy", json!("s3cr3t-1")).await;
    set(&app, &id, "plain", json!("hello")).await;
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(json!({ "item": "send.bru", "env": "local" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let dto = list(&app, &id, Some("local")).await;
    assert_eq!(names(&dto), ["copy", "docId", "plain", "token"]);
    for masked in ["token", "docId", "copy"] {
        let v = var(&dto, masked);
        assert_eq!(v["masked"], true, "{masked}: {v}");
        assert_eq!(v["value"], Value::Null);
        assert_eq!(v["source"], "runtime");
    }
    assert_eq!(var(&dto, "plain")["value"], "hello");
    assert_eq!(var(&dto, "plain")["shadows"], json!([]));
    let text = dto.to_string();
    for secret in ["\"x1\"", "abc12345", "s3cr3t-1"] {
        assert!(!text.contains(secret), "{text}");
    }
}

#[tokio::test]
async fn list_truncates_at_1kib() {
    let dir = small_collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    set(&app, &id, "long", json!("é".repeat(1024))).await;
    let dto = list(&app, &id, None).await;
    let v = var(&dto, "long");
    assert_eq!(v["truncated"], true);
    assert_eq!(v["value"].as_str().unwrap().len(), 1024);
}

#[tokio::test]
async fn delete_one_is_idempotent_and_next_send_misses_it() {
    let mock = mock().await;
    let dir = small_collection(&mock.uri());
    write(
        dir.path(),
        "q.bru",
        "meta {\n  name: q\n  type: http\n  seq: 5\n}\n\nget {\n  url: {{host}}/q?a={{a}}\n}\n",
    );
    let app = common::app();
    let id = open(&app, dir.path()).await;
    set(&app, &id, "a", json!("one")).await;
    for _ in 0..2 {
        let (status, _) = call(
            &app,
            "DELETE",
            &format!("/api/collections/{id}/runtime-vars/a"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    assert!(names(&list(&app, &id, None).await).is_empty());
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(json!({ "item": "q.bru", "env": "local" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let got = mock.received_requests().await.unwrap();
    assert!(
        !got.last().unwrap().url.as_str().contains("a=one"),
        "{}",
        got.last().unwrap().url
    );
}

#[tokio::test]
async fn clear_all_then_list_empty() {
    let dir = small_collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    set(&app, &id, "a", json!(1)).await;
    set(&app, &id, "b", json!(2)).await;
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/api/collections/{id}/runtime-vars"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(names(&list(&app, &id, None).await).is_empty());
}

#[tokio::test]
async fn routes_need_session_token() {
    let dir = small_collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    for (method, uri) in [
        ("GET", format!("/api/collections/{id}/runtime-vars")),
        ("DELETE", format!("/api/collections/{id}/runtime-vars")),
        ("DELETE", format!("/api/collections/{id}/runtime-vars/a")),
    ] {
        let req = Request::builder()
            .method(method)
            .uri(&uri)
            .header("host", format!("127.0.0.1:{}", common::PORT))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn next_json(socket: &mut Socket) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), socket.next())
            .await
            .expect("timed out waiting for an event")
            .expect("socket closed")
            .unwrap();
        if let Message::Text(text) = msg {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

/// Review Focus 4: clearing while a run waits keeps only what the run itself set.
#[tokio::test]
async fn clear_during_run_keeps_only_run_changes() {
    let mock = MockServer::start().await;
    Mock::given(path_regex("^/slow$"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(600)))
        .mount(&mock)
        .await;
    Mock::given(path_regex(".*"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock)
        .await;
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "bruno.json",
        r#"{"version":"1","name":"rv","type":"collection"}"#,
    );
    write(
        dir.path(),
        "1-slow.bru",
        &format!(
            "meta {{\n  name: slow\n  type: http\n  seq: 1\n}}\n\nget {{\n  url: {}/slow\n}}\n",
            mock.uri()
        ),
    );
    write(
        dir.path(),
        "2-late.bru",
        &format!(
            "meta {{\n  name: late\n  type: http\n  seq: 2\n}}\n\nget {{\n  url: {}/late\n}}\n\nscript:post-response {{\n  bru.setVar(\"late\", 1);\n}}\n",
            mock.uri()
        ),
    );
    let (running, fut): (RunningServer, _) = start(
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
    tokio::spawn(fut);
    let id = running.workspace.open(dir.path()).unwrap();
    let open = running.workspace.get(&id).unwrap();
    open.set_runtime_var("before", json!("x"));
    let mut req = format!("ws://{}/api/ws", running.addr)
        .into_client_request()
        .unwrap();
    req.headers_mut().insert(
        "sec-websocket-protocol",
        format!("plover.token.{}", running.token).parse().unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let msg = json!({ "type": "start", "runId": "r1", "collectionId": id.0, "item": "" });
    socket.send(Message::text(msg.to_string())).await.unwrap();
    assert_eq!(next_json(&mut socket).await["type"], "accepted");
    assert_eq!(next_json(&mut socket).await["type"], "started");
    open.clear_runtime();
    loop {
        let event = next_json(&mut socket).await;
        if event["type"] == "finished" || event["type"] == "error" {
            assert_eq!(event["type"], "finished", "{event}");
            break;
        }
    }
    let runtime = open.runtime();
    assert_eq!(runtime.keys().collect::<Vec<_>>(), ["late"], "{runtime:?}");
}
