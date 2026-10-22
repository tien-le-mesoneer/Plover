mod common;

use std::fs;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use wiremock::matchers::{header, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ECHO_JSON: &str = "echo/echo json.bru";

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
async fn open_and_tree() {
    let (_dir, root) = fixture_copy();
    let app = app();
    let id = open(&app, &root).await;
    let (status, list) = call(&app, "GET", "/api/collections", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list[0]["name"], "bruno-testbench");

    let (status, tree) = call(&app, "GET", &format!("/api/collections/{id}/tree"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tree["kind"], "folder");
    assert_eq!(tree["id"], "");
    let children = tree["children"].as_array().unwrap();
    assert!(children.len() >= 10, "{}", children.len());
    // Folders come before requests (Bruno's order).
    let first_request = children.iter().position(|c| c["kind"] != "folder").unwrap();
    assert!(
        children[first_request..]
            .iter()
            .all(|c| c["kind"] != "folder")
    );
    let ping = children.iter().find(|c| c["id"] == "ping.bru").unwrap();
    assert_eq!(ping["kind"], "request");
    assert_eq!(ping["method"], "GET");
    let echo = children.iter().find(|c| c["id"] == "echo").unwrap();
    assert!(
        echo["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == ECHO_JSON)
    );
}

#[tokio::test]
async fn read_edit_save_minimal_diff() {
    let (_dir, root) = fixture_copy();
    let app = app();
    let id = open(&app, &root).await;
    let before = fs::read_to_string(root.join(ECHO_JSON)).unwrap();
    let uri = format!("/api/collections/{id}/requests?item={}", enc(ECHO_JSON));
    let (status, read) = call(&app, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    let mut request = read["request"].clone();
    assert_eq!(request["method"], "POST");
    assert_eq!(request["body"]["mode"], "json");
    assert_eq!(request["headers"][0]["name"], "foo");
    request["headers"][0]["value"] = json!("baz");
    let (status, saved) = call(
        &app,
        "PUT",
        &uri,
        Some(json!({ "request": request, "hash": read["hash"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let after = fs::read_to_string(root.join(ECHO_JSON)).unwrap();
    assert_eq!(changed_lines(&before, &after), (1, 1), "{after}");
    assert_ne!(saved["hash"], read["hash"]);
    // The saved hash is the next expected hash.
    let (status, again) = call(
        &app,
        "PUT",
        &uri,
        Some(json!({ "request": request, "hash": saved["hash"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(fs::read_to_string(root.join(ECHO_JSON)).unwrap(), after);
}

#[tokio::test]
async fn save_conflict_409() {
    let (_dir, root) = fixture_copy();
    let app = app();
    let id = open(&app, &root).await;
    let uri = format!("/api/collections/{id}/requests?item={}", enc(ECHO_JSON));
    let (_, read) = call(&app, "GET", &uri, None).await;
    let external = fs::read_to_string(root.join(ECHO_JSON))
        .unwrap()
        .replace("foo: bar", "foo: external");
    fs::write(root.join(ECHO_JSON), &external).unwrap();
    let (status, body) = call(
        &app,
        "PUT",
        &uri,
        Some(json!({ "request": read["request"], "hash": read["hash"] })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "conflict");
    assert_eq!(body["current"]["text"], external);
    assert!(body["current"]["hash"].as_str().unwrap().len() == 64);
    assert_eq!(fs::read_to_string(root.join(ECHO_JSON)).unwrap(), external);
}

#[tokio::test]
async fn send_draft_returns_response() {
    let server = MockServer::start().await;
    Mock::given(path("/draft"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not here"))
        .expect(1)
        .mount(&server)
        .await;
    let dir = small_collection(&server.uri());
    let app = app();
    let id = open(&app, dir.path()).await;
    let (_, read) = call(
        &app,
        "GET",
        &format!("/api/collections/{id}/requests?item=a.bru"),
        None,
    )
    .await;
    let mut draft = read["request"].clone();
    draft["url"] = json!("{{host}}/draft");
    let (status, res) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(json!({ "item": "a.bru", "draft": draft, "env": "local" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(res["status"], 404);
    assert_eq!(res["bodyText"], "not here");
    assert_eq!(res["size"], 8);
    assert_eq!(res["truncated"], false);
    assert!(res["elapsedMs"].is_number());
}

#[tokio::test]
async fn send_transport_failure_is_502() {
    let dir = small_collection("http://127.0.0.1:9");
    let app = app();
    let id = open(&app, dir.path()).await;
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(json!({ "item": "a.bru", "env": "local" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"], "send_failed");
}

#[tokio::test]
async fn binary_and_large_body() {
    const SIZE: usize = 12 * 1024 * 1024;
    let server = MockServer::start().await;
    Mock::given(path("/a"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(vec![0xffu8; SIZE], "application/octet-stream"),
        )
        .mount(&server)
        .await;
    let dir = small_collection(&server.uri());
    let app = app();
    let id = open(&app, dir.path()).await;
    let (status, res) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(json!({ "item": "a.bru", "env": "local" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(res["bodyText"].is_null());
    let b64 = res["bodyBase64"].as_str().unwrap();
    assert_eq!(b64.len(), (10 * 1024 * 1024usize).div_ceil(3) * 4);
    assert_eq!(res["truncated"], true);
    assert_eq!(res["size"], SIZE);
}

#[tokio::test]
async fn bad_item_400() {
    let (_dir, root) = fixture_copy();
    let app = app();
    let id = open(&app, &root).await;
    for item in ["../x.bru", "/etc/passwd", "%2e%2e/x.bru"] {
        let (status, body) = call(
            &app,
            "GET",
            &format!("/api/collections/{id}/requests?item={item}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{item}: {body}");
        assert_eq!(body["error"], "bad_item");
    }
    let (status, _) = call(
        &app,
        "GET",
        &format!("/api/collections/{id}/requests?item=nope.bru"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, "GET", "/api/collections/ffff/tree", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn env_edit_and_secret_memory_only() {
    let server = MockServer::start().await;
    Mock::given(path("/a"))
        .and(header("x-token", "s3cr3t-value"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let dir = small_collection("http://127.0.0.1:9");
    let app = app();
    let id = open(&app, dir.path()).await;
    let env_path = dir.path().join("environments/local.bru");

    let (status, envs) = call(
        &app,
        "GET",
        &format!("/api/collections/{id}/environments"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let local = envs
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "local")
        .unwrap()
        .clone();
    assert_eq!(local["secretNames"], json!(["token"]));
    let before = fs::read_to_string(&env_path).unwrap();
    let mut vars = local["vars"].clone();
    vars[0]["value"] = json!(server.uri());
    let (status, saved) = call(
        &app,
        "PUT",
        &format!("/api/collections/{id}/environments/local"),
        Some(json!({ "vars": vars, "hash": local["hash"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let after = fs::read_to_string(&env_path).unwrap();
    assert_eq!(changed_lines(&before, &after), (1, 1));

    let (status, _) = call(
        &app,
        "PUT",
        &format!("/api/collections/{id}/environments/local/secrets"),
        Some(json!({ "values": { "token": "s3cr3t-value" } })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, res) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(json!({ "item": "a.bru", "env": "local" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(res["status"], 200);
    assert!(
        !fs::read_to_string(&env_path)
            .unwrap()
            .contains("s3cr3t-value")
    );

    let (status, _) = call(
        &app,
        "PUT",
        &format!("/api/collections/{id}/environments/nope/secrets"),
        Some(json!({ "values": {} })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unknown_api_route_is_json_404() {
    let (status, body): (StatusCode, Value) = call(&app(), "GET", "/api/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");
}
