//! Native features (folder picker, opening a browser) reach the UI through the API;
//! without a native host (web mode) those routes answer 501.

mod common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{PORT, TOKEN, call};
use plover_core::executor::ExecOptions;
use plover_server::native::{HostFuture, NativeHost};
use plover_server::{AllowList, AppState, router};
use serde_json::json;
use tower::ServiceExt;

#[derive(Default)]
struct FakeHost {
    pick: Mutex<Vec<Option<PathBuf>>>,
    opened: Mutex<Vec<String>>,
    saved: Mutex<Vec<(String, Vec<u8>)>>,
}

impl NativeHost for FakeHost {
    fn pick_folder<'a>(&'a self, _title: &'a str) -> HostFuture<'a, Option<PathBuf>> {
        Box::pin(async move { self.pick.lock().unwrap().remove(0) })
    }
    fn open_external<'a>(&'a self, url: &'a str) -> HostFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.opened.lock().unwrap().push(url.to_owned());
            Ok(())
        })
    }
    fn save_file<'a>(
        &'a self,
        default_name: &'a str,
        bytes: Vec<u8>,
    ) -> HostFuture<'a, Result<Option<PathBuf>, String>> {
        Box::pin(async move {
            self.saved
                .lock()
                .unwrap()
                .push((default_name.to_owned(), bytes));
            Ok(Some(PathBuf::from("/saved").join(default_name)))
        })
    }
}

const BASE_URL: &str = "http://127.0.0.1:4321";

fn desktop_app(host: Arc<FakeHost>) -> axum::Router {
    router(
        AppState::new(ExecOptions::default(), TOKEN, AllowList::new(PORT, &[]))
            .with_native(host)
            .with_base_url(BASE_URL),
    )
}

#[tokio::test]
async fn capabilities_web_mode() {
    let (status, body) = call(&common::app(), "GET", "/api/capabilities", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({
            "desktop": false,
            "nativePicker": false,
            "openExternal": false,
            "nativeSave": false,
            "secretBackend": "memory"
        })
    );
}

#[tokio::test]
async fn capabilities_desktop_mode() {
    let app = desktop_app(Arc::new(FakeHost::default()));
    let (status, body) = call(&app, "GET", "/api/capabilities", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["desktop"], json!(true));
    assert_eq!(body["nativePicker"], json!(true));
    assert_eq!(body["openExternal"], json!(true));
    assert_eq!(body["nativeSave"], json!(true));
}

#[tokio::test]
async fn native_save_without_host_is_501() {
    let (status, body) = call(
        &common::app(),
        "POST",
        "/api/native/save-report",
        Some(json!({"collectionId": "x", "runId": "y", "format": "html"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(body["error"], json!("not_supported"));
}

#[tokio::test]
async fn pick_folder_501_without_host() {
    let app = common::app();
    let (status, body) = call(&app, "POST", "/api/native/pick-folder", Some(json!({}))).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(body["error"], json!("not_supported"));
    let (status, _) = call(&app, "POST", "/api/native/open-browser", Some(json!({}))).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn pick_folder_returns_path_or_null() {
    let host = Arc::new(FakeHost::default());
    *host.pick.lock().unwrap() = vec![Some(PathBuf::from("/tmp/coll")), None];
    let app = desktop_app(host);
    let (status, body) = call(&app, "POST", "/api/native/pick-folder", Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "path": "/tmp/coll" }));
    let (_, body) = call(&app, "POST", "/api/native/pick-folder", Some(json!({}))).await;
    assert_eq!(body, json!({ "path": null }), "cancel is not an error");
}

#[tokio::test]
async fn open_browser_uses_a_launch_code_not_the_token() {
    let host = Arc::new(FakeHost::default());
    let app = desktop_app(host.clone());
    let (status, _) = call(&app, "POST", "/api/native/open-browser", Some(json!({}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let opened = host.opened.lock().unwrap();
    assert_eq!(opened.len(), 1);
    assert!(
        opened[0].starts_with(&format!("{BASE_URL}/#code=")),
        "{}",
        opened[0]
    );
    assert!(!opened[0].contains(TOKEN));
}

#[tokio::test]
async fn native_routes_require_token() {
    let app = desktop_app(Arc::new(FakeHost::default()));
    for (method, uri) in [
        ("GET", "/api/capabilities"),
        ("POST", "/api/native/pick-folder"),
        ("POST", "/api/native/open-browser"),
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
