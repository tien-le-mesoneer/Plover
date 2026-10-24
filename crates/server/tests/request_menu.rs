//! The URL bar's `⋯` menu: "Copy resolved URL" and "Reveal .bru file".

mod common;

use axum::http::StatusCode;
use common::{call, small_collection, write};
use serde_json::json;

async fn open(app: &axum::Router, root: &std::path::Path) -> String {
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

#[tokio::test]
async fn resolved_url_masks_secrets() {
    let dir = small_collection("https://example.test");
    write(
        dir.path(),
        "environments/local.bru",
        "vars {\n  host: https://example.test\n}\nvars:secret [\n  apiKey\n]\n",
    );
    write(
        dir.path(),
        "k.bru",
        "meta {\n  name: k\n  type: http\n  seq: 9\n}\n\nget {\n  url: {{host}}/a?k={{apiKey}}\n}\n",
    );
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (status, set) = call(
        &app,
        "PUT",
        &format!("/api/collections/{id}/environments/local/secrets"),
        Some(json!({ "values": { "apiKey": "s3cr3t-value" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/resolved-url"),
        Some(json!({ "item": "k.bru", "env": "local" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["url"], "https://example.test/a?k=***");
    assert_eq!(body["masked"], true);
    assert!(!body.to_string().contains("s3cr3t-value"));

    // Without the environment nothing is filled in or masked.
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/resolved-url"),
        Some(json!({ "item": "a.bru" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["masked"], false);
}

#[tokio::test]
async fn resolved_url_scratch() {
    let dir = small_collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (_, read) = call(
        &app,
        "GET",
        &format!("/api/collections/{id}/requests?item=a.bru"),
        None,
    )
    .await;
    let mut draft = read["request"].clone();
    draft["url"] = json!("{{host}}/scratch");
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/resolved-url"),
        Some(json!({ "item": null, "draft": draft, "env": "local" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["url"], "https://example.test/scratch");
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/resolved-url"),
        Some(json!({ "item": null })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "bad_item");
}

#[tokio::test]
async fn reveal_in_web_mode_is_not_supported() {
    let dir = small_collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (status, body) = call(
        &app,
        "POST",
        "/api/native/reveal",
        Some(json!({ "collection": id, "item": "a.bru" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{body}");
    let (_, caps) = call(&app, "GET", "/api/capabilities", None).await;
    assert_eq!(caps["reveal"], false);
}
