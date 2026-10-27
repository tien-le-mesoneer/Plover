//! `POST /collections/{id}/resolve-text`: `{{variables}}` filled in for copying, with
//! secrets kept out of the response until the caller asks again with `reveal: true`.

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
        r#"{"version":"1","name":"resolve","type":"collection"}"#,
    );
    write(
        root,
        "environments/dev.bru",
        "vars {\n  host: api.test\n}\nvars:secret [\n  apiKey\n]\n",
    );
    write(
        root,
        "auth/login.bru",
        &format!(
            "meta {{\n  name: login\n  type: http\n  seq: 1\n}}\n\nget {{\n  url: {base}/login\n}}\n\nscript:post-response {{\n  bru.setVar(\"token\", \"tok-RESOLVE-9876\");\n}}\n"
        ),
    );
    write(
        root,
        "me.bru",
        &format!(
            "meta {{\n  name: me\n  type: http\n  seq: 2\n}}\n\nget {{\n  url: {base}/me\n}}\n\nheaders {{\n  Authorization: Bearer {{{{token}}}}\n}}\n"
        ),
    );
    dir
}

async fn resolve(
    app: &axum::Router,
    id: &str,
    text: &str,
    reveal: bool,
) -> (StatusCode, Value, String) {
    let (status, body) = call(
        app,
        "POST",
        &format!("/api/collections/{id}/resolve-text"),
        Some(json!({ "item": "me.bru", "env": "dev", "text": text, "reveal": reveal })),
    )
    .await;
    let raw = body.to_string();
    (status, body, raw)
}

async fn set_secret(app: &axum::Router, id: &str) {
    let (status, _) = call(
        app,
        "PUT",
        &format!("/api/collections/{id}/environments/dev/secrets"),
        Some(json!({ "values": { "apiKey": "key-SECRET-5555" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn resolves_plain_var() {
    let dir = collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (status, body, _) = resolve(&app, &id, "{{host}}", false).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "text": "api.test", "containsSecret": false }));
}

#[tokio::test]
async fn secret_without_reveal_returns_null() {
    let dir = collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    set_secret(&app, &id).await;
    let (status, body, raw) = resolve(&app, &id, "k={{apiKey}}", false).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "text": null, "containsSecret": true }));
    assert!(!raw.contains("key-SECRET-5555"));
}

#[tokio::test]
async fn secret_with_reveal_returns_value() {
    let dir = collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    set_secret(&app, &id).await;
    let (_, body, _) = resolve(&app, &id, "k={{apiKey}}", true).await;
    assert_eq!(
        body,
        json!({ "text": "k=key-SECRET-5555", "containsSecret": true })
    );
}

#[tokio::test]
async fn sent_credential_runtime_var_counts_as_secret() {
    let mock = MockServer::start().await;
    for p in ["/login", "/me"] {
        Mock::given(path(p))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock)
            .await;
    }
    let dir = collection(&mock.uri());
    let app = common::app();
    let id = open(&app, dir.path()).await;
    for item in ["auth/login.bru", "me.bru"] {
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/send"),
            Some(json!({ "item": item, "env": "dev" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let (_, body, raw) = resolve(&app, &id, "{{token}}", false).await;
    assert_eq!(body, json!({ "text": null, "containsSecret": true }));
    assert!(!raw.contains("tok-RESOLVE-9876"));
}

#[tokio::test]
async fn process_env_counts_as_secret() {
    let dir = collection("https://example.test");
    write(dir.path(), ".env", "DB_PASS=hunter2-resolve\n");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (_, body, raw) = resolve(&app, &id, "{{process.env.DB_PASS}}", false).await;
    assert_eq!(body, json!({ "text": null, "containsSecret": true }));
    assert!(!raw.contains("hunter2-resolve"));
}

#[tokio::test]
async fn undefined_var_is_left_as_written() {
    let dir = collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (_, body, _) = resolve(&app, &id, "{{nope}}", false).await;
    assert_eq!(body, json!({ "text": "{{nope}}", "containsSecret": false }));
}

#[tokio::test]
async fn resolves_whole_body_text() {
    let dir = collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (_, body, _) = resolve(&app, &id, r#"{"a":"{{host}}","b":"{{host}}"}"#, false).await;
    assert_eq!(body["text"], r#"{"a":"api.test","b":"api.test"}"#);
}

#[tokio::test]
async fn text_over_1_mib_is_400() {
    let dir = collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (status, body, _) = resolve(&app, &id, &"x".repeat((1 << 20) + 1), false).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "text_too_large");
}
