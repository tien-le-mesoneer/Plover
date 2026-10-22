//! Helpers shared by the server integration tests.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use brs_core::executor::ExecOptions;
use brs_server::{AllowList, AppState, router};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

pub const PORT: u16 = 4321;
pub const TOKEN: &str = "test-token";
pub const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/bruno-tests/collection"
);

pub fn app() -> axum::Router {
    router(AppState::new(
        ExecOptions::default(),
        TOKEN,
        AllowList::new(PORT, &[]),
    ))
}

pub async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", format!("127.0.0.1:{PORT}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json");
    let req = match body {
        Some(b) => req.body(Body::from(b.to_string())).unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, value)
}

pub fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_dir(&entry.path(), &target);
        } else if kind.is_file() {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub fn fixture_copy() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("collection");
    copy_dir(Path::new(FIXTURE), &root);
    (dir, root)
}

pub fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

/// A minimal collection; `host` comes from environment `local`.
pub fn small_collection(host: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"small","type":"collection"}"#,
    );
    write(
        root,
        "environments/local.bru",
        &format!("vars {{\n  host: {host}\n}}\nvars:secret [\n  token\n]\n"),
    );
    for (i, name) in ["a", "b", "c"].iter().enumerate() {
        write(
            root,
            &format!("{name}.bru"),
            &format!(
                "meta {{\n  name: {name}\n  type: http\n  seq: {}\n}}\n\nget {{\n  url: {{{{host}}}}/{name}\n}}\n\nheaders {{\n  x-token: {{{{token}}}}\n}}\n",
                i + 1
            ),
        );
    }
    dir
}

pub fn changed_lines(before: &str, after: &str) -> (usize, usize) {
    let diff = similar::TextDiff::from_lines(before, after);
    let mut removed = 0;
    let mut added = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Delete => removed += 1,
            similar::ChangeTag::Insert => added += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    (removed, added)
}

pub fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
