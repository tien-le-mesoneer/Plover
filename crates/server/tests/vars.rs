//! `POST /collections/{id}/vars`: where each variable comes from, with masking.

mod common;

use std::path::Path;

use axum::http::StatusCode;
use common::{call, write};
use serde_json::{Value, json};
use wiremock::matchers::path;
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

fn collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"vars","type":"collection"}"#,
    );
    write(
        root,
        "collection.bru",
        "vars:pre-request {\n  host: from-collection\n  plain: prefix-abc12345\n}\n",
    );
    write(
        root,
        "environments/dev.bru",
        "vars {\n  host: from-env\n}\nvars:secret [\n  apiKey\n]\n",
    );
    write(
        root,
        "auth/login.bru",
        &format!(
            "meta {{\n  name: login\n  type: http\n  seq: 1\n}}\n\nget {{\n  url: {base}/login\n}}\n\nscript:post-response {{\n  bru.setVar(\"token\", \"tok-AAAABBBBCCCC\");\n  bru.setVar(\"other\", \"visible-value-1234\");\n}}\n"
        ),
    );
    write(
        root,
        "me.bru",
        &format!(
            "meta {{\n  name: me\n  type: http\n  seq: 2\n}}\n\nget {{\n  url: {base}/me\n}}\n\nheaders {{\n  Authorization: Bearer {{{{token}}}}\n}}\n\nscript:pre-request {{\n  bru.setVar(\"fromPre\", 1);\n}}\n"
        ),
    );
    dir
}

async fn vars(app: &axum::Router, id: &str, body: Value) -> Value {
    let (status, body) = call(
        app,
        "POST",
        &format!("/api/collections/{id}/vars"),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
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
    Mock::given(path("/login"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock)
        .await;
    Mock::given(path("/me"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock)
        .await;
    mock
}

#[tokio::test]
async fn vars_lists_sources_and_shadows() {
    let dir = collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let dto = vars(&app, &id, json!({ "item": "me.bru", "env": "dev" })).await;
    let host = var(&dto, "host");
    assert_eq!(host["source"], "environment");
    assert_eq!(host["value"], "from-env");
    assert_eq!(host["masked"], false);
    assert_eq!(host["shadows"], json!(["collection"]));
}

#[tokio::test]
async fn vars_masks_keychain_secret() {
    let dir = collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (status, _) = call(
        &app,
        "PUT",
        &format!("/api/collections/{id}/environments/dev/secrets"),
        Some(json!({ "values": { "apiKey": "abc12345" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let dto = vars(&app, &id, json!({ "item": "me.bru", "env": "dev" })).await;
    let key = var(&dto, "apiKey");
    assert_eq!(key["masked"], true);
    assert_eq!(key["value"], Value::Null);
    assert!(!dto.to_string().contains("\"abc12345\""), "{dto}");
    // Review Focus 3: masked per variable, not by substring.
    assert_eq!(var(&dto, "plain")["value"], "prefix-abc12345");
}

#[tokio::test]
async fn vars_masks_process_env() {
    let dir = collection("https://example.test");
    write(dir.path(), ".env", "DB_PASS=hunter2-local\n");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let dto = vars(&app, &id, json!({ "item": "me.bru" })).await;
    let pass = var(&dto, "process.env.DB_PASS");
    assert_eq!(pass["source"], "processEnv");
    assert_eq!(pass["masked"], true);
    assert!(!dto.to_string().contains("hunter2-local"));
}

#[tokio::test]
async fn vars_masks_runtime_value_sent_as_bearer() {
    let mock = mock().await;
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    for item in ["auth/login.bru", "me.bru"] {
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/send"),
            Some(json!({ "item": item })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let dto = vars(&app, &id, json!({ "item": "me.bru" })).await;
    assert_eq!(var(&dto, "token")["masked"], true);
    assert_eq!(var(&dto, "token")["source"], "runtime");
    assert!(!dto.to_string().contains("tok-AAAABBBBCCCC"));
    assert_eq!(var(&dto, "other")["value"], "visible-value-1234");
}

#[tokio::test]
async fn vars_cuts_to_1kib() {
    let dir = collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let long = "é".repeat(1024); // 2 KiB
    let (status, _) = call(
        &app,
        "PUT",
        &format!("/api/collections/{id}/runtime-vars/long"),
        Some(json!({ "value": long })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let dto = vars(&app, &id, json!({ "item": "me.bru" })).await;
    let v = var(&dto, "long");
    assert_eq!(v["truncated"], true);
    assert_eq!(v["value"].as_str().unwrap().len(), 1024);
}

#[tokio::test]
async fn vars_lists_set_later() {
    let dir = collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let dto = vars(&app, &id, json!({ "item": "me.bru" })).await;
    let later = dto["setLater"].as_array().unwrap();
    assert!(
        later.contains(&json!({ "name": "token", "by": "auth/login.bru" })),
        "{dto}"
    );
    assert!(
        later.contains(&json!({ "name": "fromPre", "by": "me.bru" })),
        "{dto}"
    );
    // Names `vars` already defines are not "set later".
    assert!(later.iter().all(|l| l["name"] != "host"));
}

#[tokio::test]
async fn vars_accepts_null_item_with_draft() {
    let dir = collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (_, stored) = call(
        &app,
        "GET",
        &format!("/api/collections/{id}/requests?item=me.bru"),
        None,
    )
    .await;
    let dto = vars(
        &app,
        &id,
        json!({ "item": null, "draft": stored["request"] }),
    )
    .await;
    assert_eq!(var(&dto, "host")["source"], "collection");
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/vars"),
        Some(json!({ "item": null })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn vars_runs_no_script_and_sends_nothing() {
    let mock = MockServer::start().await;
    Mock::given(path("/me"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&mock)
        .await;
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let dto = vars(&app, &id, json!({ "item": "me.bru" })).await;
    assert!(
        dto["vars"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["name"] != "fromPre")
    );
    let again = vars(&app, &id, json!({ "item": "me.bru" })).await;
    assert!(
        again["vars"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["name"] != "fromPre")
    );
}

#[tokio::test]
async fn vars_needs_token() {
    let dir = collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/collections/{id}/vars"))
        .header("host", format!("127.0.0.1:{}", common::PORT))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"item":"me.bru"}"#))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn run_results_feed_sent_credentials() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let mock = mock().await;
    let dir = collection(&mock.uri());
    let state = tempfile::tempdir().unwrap();
    let (running, fut) = plover_server::start(
        plover_server::ServerConfig {
            exec: plover_core::executor::ExecOptions::default(),
            state_dir: Some(state.path().to_path_buf()),
            ..plover_server::ServerConfig::default()
        },
        std::future::pending(),
    )
    .await
    .unwrap();
    tokio::spawn(fut);
    let client = reqwest::Client::new();
    let base = format!("http://{}/api", running.addr);
    let post = |url: String, body: Value| {
        let req = client
            .post(url)
            .bearer_auth(&running.token)
            .header("content-type", "application/json")
            .body(body.to_string());
        async move {
            let bytes = req.send().await.unwrap().bytes().await.unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        }
    };
    let opened = post(format!("{base}/collections"), json!({ "path": dir.path() })).await;
    let id = opened["id"].as_str().unwrap().to_owned();
    let mut req = format!("ws://{}/api/ws", running.addr)
        .into_client_request()
        .unwrap();
    req.headers_mut().insert(
        "sec-websocket-protocol",
        format!("plover.token.{}", running.token).parse().unwrap(),
    );
    let mut socket = tokio_tungstenite::connect_async(req).await.unwrap().0;
    let start =
        json!({ "type": "start", "runId": "r1", "collectionId": id, "item": "", "env": null });
    socket.send(Message::text(start.to_string())).await.unwrap();
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(10), socket.next())
            .await
            .expect("run timed out")
            .unwrap()
            .unwrap();
        if let Message::Text(text) = msg {
            let e: Value = serde_json::from_str(&text).unwrap();
            if e["type"] == "finished" || e["type"] == "error" {
                assert_eq!(e["type"], "finished", "{e}");
                break;
            }
        }
    }
    let dto = post(
        format!("{base}/collections/{id}/vars"),
        json!({ "item": "me.bru" }),
    )
    .await;
    assert_eq!(var(&dto, "token")["masked"], true, "{dto}");
    assert!(!dto.to_string().contains("tok-AAAABBBBCCCC"));
}
