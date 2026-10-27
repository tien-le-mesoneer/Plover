//! Remembered collections: restore on start, close, recent list and per-collection UI state.

mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{PORT, TOKEN, call, fixture_copy};
use plover_core::app_state::AppStateStore;
use plover_core::executor::ExecOptions;
use plover_server::{AllowList, AppState, router};
use serde_json::{Value, json};
use tower::ServiceExt;

fn state_with(dir: &Path) -> AppState {
    let state = AppState::new(ExecOptions::default(), TOKEN, AllowList::new(PORT, &[]));
    state
        .workspace
        .set_app_state(Arc::new(AppStateStore::load(dir)));
    state
}

async fn open(app: &axum::Router, path: &Path) -> String {
    let (status, body) = call(
        app,
        "POST",
        "/api/collections",
        Some(json!({ "path": path })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn restart_restores_open_collections_and_lists_missing_ones() {
    let state_dir = tempfile::tempdir().unwrap();
    let (_a_dir, a) = fixture_copy();
    let (_b_dir, b) = fixture_copy();
    fs::write(
        b.join("bruno.json"),
        r#"{"version":"1","name":"second","type":"collection"}"#,
    )
    .unwrap();
    let first = router(state_with(state_dir.path()));
    let a_id = open(&first, &a).await;
    let b_id = open(&first, &b).await;
    fs::remove_dir_all(&b).unwrap();

    let state = state_with(state_dir.path());
    state.workspace.restore();
    let app = router(state);
    let (status, list) = call(&app, "GET", "/api/collections", None).await;
    assert_eq!(status, StatusCode::OK);
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 2, "{list:?}");
    assert_eq!(list[0]["id"], a_id);
    assert_eq!(list[0]["available"], true);
    assert_eq!(list[1]["id"], b_id);
    assert_eq!(list[1]["available"], false);
    assert_eq!(list[1]["name"], "second");
    assert!(list[1]["error"].as_str().is_some_and(|e| !e.is_empty()));

    // Removing the unavailable entry forgets it.
    let (status, _) = call(&app, "DELETE", &format!("/api/collections/{b_id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = call(&app, "GET", "/api/collections", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn close_keeps_recent_and_recent_can_be_removed() {
    let state_dir = tempfile::tempdir().unwrap();
    let (_dir, root) = fixture_copy();
    let app = router(state_with(state_dir.path()));
    let id = open(&app, &root).await;

    let (status, _) = call(&app, "DELETE", &format!("/api/collections/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = call(&app, "GET", "/api/collections", None).await;
    assert!(list.as_array().unwrap().is_empty());
    let (status, _) = call(&app, "DELETE", &format!("/api/collections/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, recent) = call(&app, "GET", "/api/recent", None).await;
    assert_eq!(status, StatusCode::OK);
    let recent = recent.as_array().unwrap().clone();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0]["name"], "bruno-testbench");
    assert_eq!(recent[0]["open"], false);
    assert!(recent[0]["lastOpened"].as_u64().unwrap() > 0);
    // `path` is for display and for reopening.
    let reopened = open(&app, Path::new(recent[0]["path"].as_str().unwrap())).await;
    assert_eq!(reopened, id);

    let key = recent[0]["root"].as_str().unwrap();
    let uri = format!("/api/recent?root={}", urlencode(key));
    let (status, _) = call(&app, "DELETE", &uri, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, recent) = call(&app, "GET", "/api/recent", None).await;
    assert!(recent.as_array().unwrap().is_empty());
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[tokio::test]
async fn ui_state_round_trips_and_is_validated() {
    let state_dir = tempfile::tempdir().unwrap();
    let (_dir, root) = fixture_copy();
    let app = router(state_with(state_dir.path()));
    let id = open(&app, &root).await;
    let ui = json!({ "tabs": ["ping.bru"], "active": "ping.bru", "expanded": ["echo"], "env": "Prod", "collapsed": false });

    let (status, body) = call(
        &app,
        "PUT",
        &format!("/api/ui-state/{id}"),
        Some(ui.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, all) = call(&app, "GET", "/api/ui-state", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(all[&id], ui);

    // It survives a restart.
    let state = state_with(state_dir.path());
    state.workspace.restore();
    let (_, all) = call(&router(state), "GET", "/api/ui-state", None).await;
    assert_eq!(all[&id], ui);

    let many: Vec<String> = (0..51).map(|i| format!("r{i}.bru")).collect();
    let (status, body) = call(
        &app,
        "PUT",
        &format!("/api/ui-state/{id}"),
        Some(json!({ "tabs": many })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "bad_request");

    let huge = json!({ "expanded": vec!["x".repeat(900); 80] });
    let (status, body) = call(&app, "PUT", &format!("/api/ui-state/{id}"), Some(huge)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(body["error"], "payload_too_large");

    let (status, _) = call(
        &app,
        "PUT",
        "/api/ui-state/0000000000000000",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn without_a_state_store_nothing_is_remembered() {
    let (_dir, root) = fixture_copy();
    let app = common::app();
    let id = open(&app, &root).await;
    let (status, recent) = call(&app, "GET", "/api/recent", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recent, Value::Array(vec![]));
    let (status, _) = call(&app, "PUT", &format!("/api/ui-state/{id}"), Some(json!({}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "accepted and not kept");
}

#[tokio::test]
async fn new_routes_require_the_token() {
    let app = common::app();
    for (method, uri) in [
        ("GET", "/api/recent"),
        ("DELETE", "/api/recent?root=x"),
        ("GET", "/api/ui-state"),
        ("PUT", "/api/ui-state/abc"),
        ("DELETE", "/api/collections/abc"),
    ] {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", format!("127.0.0.1:{PORT}"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

#[tokio::test]
async fn layout_get_put() {
    let state_dir = tempfile::tempdir().unwrap();
    let app = router(state_with(state_dir.path()));
    let (status, body) = call(&app, "GET", "/api/layout", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "sidebarWidth": null }));
    let (status, body) = call(
        &app,
        "PUT",
        "/api/layout",
        Some(json!({ "sidebarWidth": 320 })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    // It survives a restart.
    let again = router(state_with(state_dir.path()));
    let (_, body) = call(&again, "GET", "/api/layout", None).await;
    assert_eq!(body, json!({ "sidebarWidth": 320 }));
    let (status, _) = call(
        &app,
        "PUT",
        "/api/layout",
        Some(json!({ "sidebarWidth": null })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn layout_rejects_out_of_range() {
    let state_dir = tempfile::tempdir().unwrap();
    let app = router(state_with(state_dir.path()));
    for w in [50, 5000] {
        let (status, body) = call(
            &app,
            "PUT",
            "/api/layout",
            Some(json!({ "sidebarWidth": w })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{w}");
        assert_eq!(body["error"], "bad_layout");
    }
    for method in ["GET", "PUT"] {
        let req = Request::builder()
            .method(method)
            .uri("/api/layout")
            .header("host", format!("127.0.0.1:{PORT}"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "{method} needs the token"
        );
    }
}

#[tokio::test]
async fn put_ui_state_round_trips_collapsed() {
    let state_dir = tempfile::tempdir().unwrap();
    let (_dir, root) = fixture_copy();
    let app = router(state_with(state_dir.path()));
    let id = open(&app, &root).await;
    let (status, body) = call(
        &app,
        "PUT",
        &format!("/api/ui-state/{id}"),
        Some(json!({ "collapsed": true })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let state = state_with(state_dir.path());
    state.workspace.restore();
    let (_, all) = call(&router(state), "GET", "/api/ui-state", None).await;
    assert_eq!(all[&id]["collapsed"], true);
}
