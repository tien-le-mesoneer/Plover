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

#[tokio::test]
async fn runtime_var_set_is_seen_by_next_send() {
    use std::io::{Read, Write};
    // A one-shot HTTP server that records the request line.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 2048];
        let n = stream.read(&mut buf).unwrap();
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}");
        String::from_utf8_lossy(&buf[..n])
            .lines()
            .next()
            .unwrap_or("")
            .to_owned()
    });
    let dir = small_collection(&format!("http://{addr}"));
    write(
        dir.path(),
        "d.bru",
        "meta {\n  name: d\n  type: http\n  seq: 9\n}\n\nget {\n  url: {{host}}/d/{{docId}}\n}\n",
    );
    let app = common::app();
    let id = open(&app, dir.path()).await;
    let (status, body) = call(
        &app,
        "PUT",
        &format!("/api/collections/{id}/runtime-vars/docId"),
        Some(json!({ "value": "d-8812" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, res) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(json!({ "item": "d.bru", "env": "local" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(seen.join().unwrap(), "GET /d/d-8812 HTTP/1.1");
}

#[tokio::test]
async fn runtime_var_bad_name_400() {
    let dir = small_collection("https://example.test");
    let app = common::app();
    let id = open(&app, dir.path()).await;
    for name in ["1abc", "a%20b", "a{b"] {
        let (status, body) = call(
            &app,
            "PUT",
            &format!("/api/collections/{id}/runtime-vars/{name}"),
            Some(json!({ "value": 1 })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {body}");
        assert_eq!(body["error"], "bad_name");
    }
}
