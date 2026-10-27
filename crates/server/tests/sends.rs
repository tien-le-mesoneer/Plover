//! Send history: every finished UI send is recorded (masked) and can be listed, shown
//! and cleared (`/api/sends`).

mod common;

use std::path::Path;
use std::time::Duration;

use axum::http::StatusCode;
use common::{PORT, TOKEN, call, write};
use plover_core::executor::ExecOptions;
use plover_core::send_history::SendHistory;
use plover_server::{AllowList, AppState, router};
use serde_json::{Value, json};
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET: &str = "sk-HISTORY-SECRET-7788";

fn app_at(state_dir: &Path) -> axum::Router {
    router(
        AppState::new(ExecOptions::default(), TOKEN, AllowList::new(PORT, &[]))
            .with_send_history(SendHistory::at(state_dir)),
    )
}

async fn mock() -> MockServer {
    let mock = MockServer::start().await;
    Mock::given(path("/me"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"ok":true}"#))
        .mount(&mock)
        .await;
    Mock::given(path("/slow"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&mock)
        .await;
    mock
}

fn collection(base: &str, name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        &format!(r#"{{"version":"1","name":"{name}","type":"collection"}}"#),
    );
    write(
        root,
        "environments/dev.bru",
        "vars {\n}\nvars:secret [\n  apiKey\n]\n",
    );
    write(
        root,
        "me.bru",
        &format!(
            "meta {{\n  name: me\n  type: http\n  seq: 1\n}}\n\npost {{\n  url: {base}/me?key={{{{apiKey}}}}\n  body: json\n}}\n\nheaders {{\n  X-Api-Key: {{{{apiKey}}}}\n}}\n\nbody:json {{\n  {{\"k\": \"{{{{apiKey}}}}\"}}\n}}\n"
        ),
    );
    write(
        root,
        "slow.bru",
        &format!(
            "meta {{\n  name: slow\n  type: http\n  seq: 2\n}}\n\nget {{\n  url: {base}/slow\n}}\n"
        ),
    );
    write(
        root,
        "down.bru",
        "meta {\n  name: down\n  type: http\n  seq: 3\n}\n\nget {\n  url: http://127.0.0.1:9/nothing\n}\n",
    );
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

async fn set_secret(app: &axum::Router, id: &str) {
    let (status, _) = call(
        app,
        "PUT",
        &format!("/api/collections/{id}/environments/dev/secrets"),
        Some(json!({ "values": { "apiKey": SECRET } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

async fn send(app: &axum::Router, id: &str, body: Value) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(body),
    )
    .await
}

async fn sends(app: &axum::Router, query: &str) -> Vec<Value> {
    let (status, body) = call(app, "GET", &format!("/api/sends{query}"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().unwrap().clone()
}

#[tokio::test]
async fn send_is_recorded_and_listed() {
    let mock = mock().await;
    let dir = collection(&mock.uri(), "one");
    let state = tempfile::tempdir().unwrap();
    let app = app_at(state.path());
    let id = open(&app, dir.path()).await;
    let (status, body) = send(&app, &id, json!({ "item": "me.bru" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("historyError").is_none(), "{body}");
    let list = sends(&app, "").await;
    assert_eq!(list.len(), 1);
    let meta = &list[0];
    assert_eq!(meta["collectionId"], id);
    assert_eq!(meta["collectionName"], "one");
    assert_eq!(meta["item"], "me.bru");
    assert_eq!(meta["method"], "POST");
    assert_eq!(meta["status"], "pass");
    assert_eq!(meta["httpStatus"], 200);
    let (status, record) = call(
        &app,
        "GET",
        &format!("/api/sends/{}", meta["id"].as_str().unwrap()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{record}");
    assert_eq!(record["meta"]["id"], meta["id"]);
    assert_eq!(record["detail"]["result"]["response"]["status"], 200);
    assert_eq!(record["detail"]["request"]["method"], "POST");
}

#[tokio::test]
async fn scratch_send_has_null_item() {
    let mock = mock().await;
    let dir = collection(&mock.uri(), "scratch");
    let state = tempfile::tempdir().unwrap();
    let app = app_at(state.path());
    let id = open(&app, dir.path()).await;
    let (_, stored) = call(
        &app,
        "GET",
        &format!("/api/collections/{id}/requests?item=slow.bru"),
        None,
    )
    .await;
    let mut draft = stored["request"].clone();
    draft["name"] = json!("scratch");
    draft["url"] = json!(format!("{}/me", mock.uri()));
    let (status, body) = send(&app, &id, json!({ "draft": draft })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let list = sends(&app, "").await;
    assert_eq!(list[0]["item"], Value::Null);
    assert_eq!(list[0]["name"], "scratch");
}

#[tokio::test]
async fn cancelled_send_is_not_recorded() {
    let mock = mock().await;
    let dir = collection(&mock.uri(), "cancel");
    let state = tempfile::tempdir().unwrap();
    let app = app_at(state.path());
    let id = open(&app, dir.path()).await;
    let running = {
        let app = app.clone();
        let id = id.clone();
        tokio::spawn(
            async move { send(&app, &id, json!({ "item": "slow.bru", "sendId": "s1" })).await },
        )
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send/s1/cancel"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = running.await.unwrap();
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(sends(&app, "").await.is_empty());
}

#[tokio::test]
async fn transport_error_is_recorded_with_error() {
    let mock = mock().await;
    let dir = collection(&mock.uri(), "down");
    let state = tempfile::tempdir().unwrap();
    let app = app_at(state.path());
    let id = open(&app, dir.path()).await;
    let (status, _) = send(&app, &id, json!({ "item": "down.bru" })).await;
    assert_ne!(status, StatusCode::OK);
    let list = sends(&app, "").await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["status"], "error");
    assert_eq!(list[0]["httpStatus"], Value::Null);
    let (_, record) = call(
        &app,
        "GET",
        &format!("/api/sends/{}", list[0]["id"].as_str().unwrap()),
        None,
    )
    .await;
    assert!(
        record["detail"]["error"]
            .as_str()
            .is_some_and(|e| !e.is_empty()),
        "{record}"
    );
}

#[tokio::test]
async fn history_response_never_contains_env_secret() {
    let mock = mock().await;
    let dir = collection(&mock.uri(), "secret");
    let state = tempfile::tempdir().unwrap();
    let app = app_at(state.path());
    let id = open(&app, dir.path()).await;
    set_secret(&app, &id).await;
    let (status, body) = send(&app, &id, json!({ "item": "me.bru", "env": "dev" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let list = sends(&app, "").await;
    let (_, record) = call(
        &app,
        "GET",
        &format!("/api/sends/{}", list[0]["id"].as_str().unwrap()),
        None,
    )
    .await;
    for text in [Value::Array(list.clone()).to_string(), record.to_string()] {
        assert!(!text.contains(SECRET), "{text}");
    }
    for entry in std::fs::read_dir(state.path().join("sends")).unwrap() {
        let bytes = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        assert!(!bytes.contains(SECRET));
    }
}

#[tokio::test]
async fn clear_by_collection() {
    let mock = mock().await;
    let a = collection(&mock.uri(), "a");
    let b = collection(&mock.uri(), "b");
    let state = tempfile::tempdir().unwrap();
    let app = app_at(state.path());
    let ida = open(&app, a.path()).await;
    let idb = open(&app, b.path()).await;
    for id in [&ida, &ida, &idb] {
        send(&app, id, json!({ "item": "me.bru" })).await;
    }
    assert_eq!(sends(&app, &format!("?collection={ida}")).await.len(), 2);
    let (status, body) = call(
        &app,
        "DELETE",
        &format!("/api/sends?collection={ida}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], 2);
    let left = sends(&app, "").await;
    assert_eq!(left.len(), 1);
    assert_eq!(left[0]["collectionId"], idb);
    let (_, body) = call(&app, "DELETE", "/api/sends", None).await;
    assert_eq!(body["removed"], 1);
}

#[tokio::test]
async fn history_write_failure_still_returns_response() {
    let mock = mock().await;
    let dir = collection(&mock.uri(), "ro");
    let state = tempfile::tempdir().unwrap();
    // A file where the state folder should be: nothing can be written under it.
    let blocked = state.path().join("blocked");
    std::fs::write(&blocked, "x").unwrap();
    let app = app_at(&blocked);
    let id = open(&app, dir.path()).await;
    let (status, body) = send(&app, &id, json!({ "item": "me.bru" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["response"]["status"], 200);
    assert!(
        body["historyError"].as_str().is_some_and(|e| !e.is_empty()),
        "{body}"
    );
}
