//! curl paste, Copy as cURL, scratch requests and HAR import through the API.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use serde_json::json;
use tower::ServiceExt;

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

async fn set_secret(app: &axum::Router, id: &str, env: &str, name: &str, value: &str) {
    let (status, body) = call(
        app,
        "PUT",
        &format!("/api/collections/{id}/environments/{env}/secrets"),
        Some(json!({ "values": { name: value } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// `small_collection` plus a folder whose auth is a bearer `{{token}}`.
fn folder_auth_collection() -> tempfile::TempDir {
    let dir = small_collection("https://api.example.test");
    write(
        dir.path(),
        "secure/folder.bru",
        "meta {\n  name: secure\n}\n\nauth {\n  mode: bearer\n}\n\nauth:bearer {\n  token: {{token}}\n}\n",
    );
    write(
        dir.path(),
        "secure/me.bru",
        "meta {\n  name: me\n  type: http\n  seq: 1\n}\n\nget {\n  url: {{host}}/me\n  auth: inherit\n}\n",
    );
    dir
}

mod curl {
    use super::*;

    #[tokio::test]
    async fn curl_parse_returns_request_and_notices() {
        let app = app();
        let (status, body) = call(
            &app,
            "POST",
            "/api/curl/parse",
            Some(json!({ "text": "curl -k -H 'x-a: 1' 'https://api.example.test/a?b=2'" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["request"]["method"], "GET");
        assert_eq!(body["request"]["url"], "https://api.example.test/a?b=2");
        assert_eq!(body["request"]["query"][0]["name"], "b");
        assert_eq!(body["request"]["headers"][0]["value"], "1");
        assert_eq!(body["request"]["auth"]["mode"], "none");
        assert_eq!(
            body["notices"],
            json!([{ "option": "-k", "reason": "`-k` not applied, TLS checking is a global setting" }])
        );
    }

    #[tokio::test]
    async fn curl_parse_invalid_is_400() {
        let app = app();
        for text in ["wget x", "curl -H 'a: b'", "curl 'unterminated"] {
            let (status, body) = call(
                &app,
                "POST",
                "/api/curl/parse",
                Some(json!({ "text": text })),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{text}: {body}");
            assert_eq!(body["error"], "invalid_curl");
        }
    }

    #[tokio::test]
    async fn copy_as_written_keeps_vars() {
        let dir = folder_auth_collection();
        let app = app();
        let id = open(&app, dir.path()).await;
        set_secret(&app, &id, "local", "token", "tok-COPY-1234").await;
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/curl"),
            Some(json!({ "item": "a.bru", "env": "local", "resolve": false })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let text = body["text"].as_str().unwrap();
        assert!(text.starts_with("curl -X GET '{{host}}/a'"), "{text}");
        assert!(text.contains("-H 'x-token: {{token}}'"), "{text}");
        assert_eq!(body["containsSecret"], false);
        assert_eq!(body["omitted"], json!([]));
    }

    #[tokio::test]
    async fn copy_filled_in_includes_folder_auth_and_flags_secret() {
        let dir = folder_auth_collection();
        let app = app();
        let id = open(&app, dir.path()).await;
        set_secret(&app, &id, "local", "token", "tok-COPY-1234").await;
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/curl"),
            Some(json!({ "item": "secure/me.bru", "env": "local", "resolve": true })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let text = body["text"].as_str().unwrap();
        assert!(text.contains("'https://api.example.test/me'"), "{text}");
        assert!(
            text.contains("-H 'Authorization: Bearer tok-COPY-1234'"),
            "{text}"
        );
        assert_eq!(body["containsSecret"], true);
        // As written, the folder auth is there with its variable.
        let (_, written) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/curl"),
            Some(json!({ "item": "secure/me.bru", "env": "local" })),
        )
        .await;
        let text = written["text"].as_str().unwrap();
        assert!(text.contains("Authorization: Bearer {{token}}"), "{text}");
        assert_eq!(written["containsSecret"], false);
    }

    #[tokio::test]
    async fn copy_unsupported_auth_is_omitted() {
        let dir = small_collection("https://api.example.test");
        let app = app();
        let id = open(&app, dir.path()).await;
        let (_, stored) = call(
            &app,
            "GET",
            &format!("/api/collections/{id}/requests?item=a.bru"),
            None,
        )
        .await;
        let mut draft = stored["request"].clone();
        draft["auth"] = json!({ "mode": "unsupported", "name": "oauth2" });
        draft["body"] = json!({ "mode": "unsupported", "name": "graphql" });
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/curl"),
            Some(json!({ "item": "a.bru", "draft": draft })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["omitted"],
            json!(["auth mode `oauth2`", "body mode `graphql`"])
        );
        assert!(body["text"].as_str().unwrap().contains("{{host}}/a"));
    }

    #[tokio::test]
    async fn copy_scratch_draft() {
        let dir = small_collection("https://api.example.test");
        let app = app();
        let id = open(&app, dir.path()).await;
        let (_, parsed) = call(
            &app,
            "POST",
            "/api/curl/parse",
            Some(json!({ "text": "curl https://x.example.test/p -d a=1" })),
        )
        .await;
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/curl"),
            Some(json!({ "item": null, "draft": parsed["request"] })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(
            body["text"]
                .as_str()
                .unwrap()
                .contains("--data-urlencode 'a=1'")
        );
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/curl"),
            Some(json!({ "item": null })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }

    #[tokio::test]
    async fn curl_routes_need_token() {
        let app = app();
        for uri in ["/api/curl/parse", "/api/collections/x/curl"] {
            let req = Request::builder()
                .method("POST")
                .uri(uri)
                .header("host", format!("127.0.0.1:{PORT}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "text": "curl x" }).to_string()))
                .unwrap();
            let res = app.clone().oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{uri}");
        }
    }
}
