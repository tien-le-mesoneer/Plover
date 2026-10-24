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

mod scratch {
    use super::*;
    use wiremock::matchers::{header, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn new_request(url: &str) -> serde_json::Value {
        json!({
            "name": "", "seq": 0.0, "kind": "http", "method": "GET", "url": url,
            "query": [], "pathParams": [], "headers": [], "body": { "mode": "none" },
            "otherBodies": [], "auth": { "mode": "inherit" }, "varsPre": [], "varsPost": [],
            "scriptPre": null, "scriptPost": null, "tests": null, "asserts": [],
            "settings": { "followRedirects": null, "maxRedirects": null, "timeoutMs": null }
        })
    }

    #[tokio::test]
    async fn create_request_then_tree_lists_it() {
        let dir = folder_auth_collection();
        let app = app();
        let id = open(&app, dir.path()).await;
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/requests/new"),
            Some(json!({ "folder": "secure", "name": "GET orders", "request": new_request("{{host}}/orders") })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["item"], "secure/GET orders.bru");
        assert_eq!(body["request"]["name"], "GET orders");
        assert_eq!(body["request"]["seq"], 2.0);
        assert!(body["hash"].as_str().unwrap().len() == 64);
        let (_, tree) = call(&app, "GET", &format!("/api/collections/{id}/tree"), None).await;
        let secure = tree["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "secure")
            .unwrap();
        assert!(
            secure["children"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == "secure/GET orders.bru")
        );
        let (status, read) = call(
            &app,
            "GET",
            &format!(
                "/api/collections/{id}/requests?item={}",
                enc("secure/GET orders.bru")
            ),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{read}");
        assert_eq!(read["hash"], body["hash"]);
    }

    #[tokio::test]
    async fn create_request_existing_is_409_exists() {
        let dir = small_collection("https://api.example.test");
        let app = app();
        let id = open(&app, dir.path()).await;
        let before = std::fs::read(dir.path().join("a.bru")).unwrap();
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/requests/new"),
            Some(json!({ "folder": "", "name": "a", "request": new_request("x") })),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["error"], "exists");
        assert_eq!(std::fs::read(dir.path().join("a.bru")).unwrap(), before);
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/requests/new"),
            Some(json!({ "folder": "../out", "name": "a", "request": new_request("x") })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"], "bad_item");
    }

    #[tokio::test]
    async fn scratch_send_null_item_uses_collection_scope() {
        let server = MockServer::start().await;
        Mock::given(path("/scratch"))
            .and(header("x-col", "1"))
            .respond_with(ResponseTemplate::new(202).set_body_string("ok"))
            .expect(1)
            .mount(&server)
            .await;
        let dir = small_collection(&server.uri());
        write(dir.path(), "collection.bru", "headers {\n  x-col: 1\n}\n");
        let app = app();
        let id = open(&app, dir.path()).await;
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/send"),
            Some(json!({ "item": null, "draft": new_request("{{host}}/scratch"), "env": "local" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["response"]["status"], 202);
    }

    #[tokio::test]
    async fn scratch_send_null_item_without_draft_is_400() {
        let dir = small_collection("https://api.example.test");
        let app = app();
        let id = open(&app, dir.path()).await;
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/send"),
            Some(json!({ "item": null })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"], "bad_item");
    }
}

mod har {
    use super::*;
    use std::fs;
    use std::path::Path;

    const SECRETS: &str = include_str!("../../core/tests/fixtures/har/secrets.har");
    const CHROME: &str = include_str!("../../core/tests/fixtures/har/chrome.har");
    const SECRET_VALUES: &[&str] = &[
        "pw-SECRET-1",
        "cs-SECRET-2",
        "basic-SECRET-3",
        "cookie-SECRET-4",
        "key-SECRET-5",
        "csrf-SECRET-6",
        "cs-SECRET-7",
        "digest-SECRET-8",
        "pw-SECRET-9",
        "tok-AAAA1111",
        "tok-BBBB2222",
        "tok-QUERY-0001",
    ];

    fn all_entries(har: &str) -> Vec<usize> {
        let v: serde_json::Value = serde_json::from_str(har).unwrap();
        (0..v["log"]["entries"].as_array().unwrap().len()).collect()
    }

    /// Every file under `root` with its bytes.
    fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    stack.push(entry.path());
                } else {
                    out.push((
                        entry.path().display().to_string(),
                        fs::read(entry.path()).unwrap(),
                    ));
                }
            }
        }
        out.sort();
        out
    }

    #[tokio::test]
    async fn har_preview_rows_and_hidden_flags() {
        let dir = small_collection("https://api.example.test");
        let app = app();
        let id = open(&app, dir.path()).await;
        let (status, rows) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/import/har/preview"),
            Some(json!({ "har": CHROME })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{rows}");
        let rows = rows.as_array().unwrap();
        let first = &rows[0];
        assert_eq!(first["index"], 0);
        assert_eq!(first["host"], "app.example.test");
        assert_eq!(first["mime"], "text/html");
        assert_eq!(first["hidden"], serde_json::Value::Null);
        assert_eq!(first["hasSecrets"], false);
        let orders = rows
            .iter()
            .find(|r| r["url"] == "https://api.example.test/v1/orders?page=1")
            .unwrap();
        assert_eq!(orders["duplicates"], 3);
        assert!(rows.iter().any(|r| r["hidden"] == "sourceMap"));
        assert!(rows.iter().any(|r| r["hidden"] == "preflight"));
        let (status, body) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/import/har/preview"),
            Some(json!({ "har": "{\"not\":\"har\"}" })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"], "invalid_har");
    }

    #[tokio::test]
    async fn har_import_writes_files_and_secrets() {
        let dir = small_collection("https://api.example.test");
        let app = app();
        let id = open(&app, dir.path()).await;
        let (status, result) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/import/har"),
            Some(json!({ "har": SECRETS, "entries": all_entries(SECRETS), "folder": "", "newFolder": "captured", "env": "local" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["failedAt"], serde_json::Value::Null);
        assert_eq!(result["env"], "local");
        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 9);
        assert!(
            items
                .iter()
                .all(|i| i.as_str().unwrap().starts_with("captured/"))
        );
        for item in items {
            assert!(dir.path().join(item.as_str().unwrap()).is_file(), "{item}");
        }
        let names: Vec<&str> = result["secretNames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        for expected in [
            "authToken",
            "authToken2",
            "authUser",
            "authPassword",
            "cookie",
            "password",
            "clientSecret",
        ] {
            assert!(
                names.contains(&expected),
                "{expected} missing from {names:?}"
            );
        }
        assert!(!result.to_string().contains("tok-AAAA1111"));

        let env_text = fs::read_to_string(dir.path().join("environments/local.bru")).unwrap();
        assert!(env_text.contains("authToken"), "{env_text}");
        let (_, envs) = call(
            &app,
            "GET",
            &format!("/api/collections/{id}/environments"),
            None,
        )
        .await;
        let local = envs
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == "local")
            .unwrap();
        let set: Vec<&str> = local["secretsSet"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert!(
            set.contains(&"authToken") && set.contains(&"cookie"),
            "{set:?}"
        );

        for (path, bytes) in snapshot(dir.path()) {
            let text = String::from_utf8_lossy(&bytes);
            for secret in SECRET_VALUES {
                assert!(!text.contains(secret), "{path} contains {secret}");
            }
        }
        let (_, tree) = call(&app, "GET", &format!("/api/collections/{id}/tree"), None).await;
        assert!(
            tree["children"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == "captured")
        );
    }

    #[tokio::test]
    async fn har_import_without_env_creates_imported_env() {
        let dir = small_collection("https://api.example.test");
        let app = app();
        let id = open(&app, dir.path()).await;
        let (status, result) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/import/har"),
            Some(json!({ "har": SECRETS, "entries": [1], "folder": "" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["env"], "Imported");
        assert!(dir.path().join("environments/Imported.bru").is_file());
        assert_eq!(
            result["secretNames"],
            json!(["accessToken", "authToken", "xCsrfToken"])
        );
        assert_eq!(result["items"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn har_import_into_new_folder() {
        let dir = small_collection("https://api.example.test");
        let app = app();
        let id = open(&app, dir.path()).await;
        for round in 0..2 {
            let (status, result) = call(
                &app,
                "POST",
                &format!("/api/collections/{id}/import/har"),
                Some(json!({ "har": CHROME, "entries": [0, 14], "folder": "", "newFolder": "captured-app.example.test" })),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{result}");
            assert_eq!(result["failedAt"], serde_json::Value::Null, "{result}");
            let items = result["items"].as_array().unwrap();
            assert_eq!(items.len(), 2);
            if round == 1 {
                // The second import goes into the same folder with new names.
                assert!(items[0].as_str().unwrap().ends_with(" 2.bru"), "{items:?}");
            }
        }
        assert!(dir.path().join("captured-app.example.test").is_dir());
        assert!(!dir.path().join("environments/Imported.bru").exists());
    }

    #[tokio::test]
    async fn har_plan_lists_secret_names_and_writes_nothing() {
        let dir = small_collection("https://api.example.test");
        let app = app();
        let id = open(&app, dir.path()).await;
        let before = snapshot(dir.path());
        let (status, plan) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/import/har/plan"),
            Some(json!({ "har": SECRETS, "entries": [3], "folder": "", "env": "local" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{plan}");
        assert_eq!(
            plan["secretNames"],
            json!(["authToken", "cookie", "xApiKey"])
        );
        assert_eq!(plan["requestNames"], json!(["GET v1-me"]));
        assert_eq!(plan["env"], "local");
        for secret in SECRET_VALUES {
            assert!(!plan.to_string().contains(secret), "{secret}");
        }
        assert_eq!(snapshot(dir.path()), before);
    }

    #[tokio::test]
    async fn har_plan_matches_import() {
        let dir = small_collection("https://api.example.test");
        let app = app();
        let id = open(&app, dir.path()).await;
        // authToken already holds a different value, so both name the token authToken2.
        set_secret(&app, &id, "local", "authToken", "something-else").await;
        let request = json!({ "har": SECRETS, "entries": [1, 2], "folder": "", "env": "local" });
        let (_, plan) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/import/har/plan"),
            Some(request.clone()),
        )
        .await;
        let (_, result) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/import/har"),
            Some(request),
        )
        .await;
        assert_eq!(plan["secretNames"], result["secretNames"]);
        let names = plan["secretNames"].as_array().unwrap();
        assert!(names.contains(&json!("authToken2")), "{names:?}");
        assert!(!names.contains(&json!("authToken")), "{names:?}");
    }

    #[tokio::test]
    async fn har_over_limit_is_413() {
        let dir = small_collection("https://api.example.test");
        let app = app();
        let id = open(&app, dir.path()).await;
        let big = format!("{{\"har\":\"{}\"}}", "a".repeat(64 * 1024 * 1024 + 16));
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/collections/{id}/import/har/preview"))
            .header("host", format!("127.0.0.1:{PORT}"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(big))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn preview_does_not_echo_response_content() {
        let dir = small_collection("https://api.example.test");
        let app = app();
        let id = open(&app, dir.path()).await;
        let content = format!("MARKER-{}", "x".repeat(64 * 1024));
        let entries: Vec<serde_json::Value> = (0..500)
            .map(|i| {
                json!({
                    "request": { "method": "GET", "url": format!("https://api.example.test/item/{i}"), "headers": [] },
                    "response": { "status": 200, "content": { "size": content.len(), "mimeType": "application/json", "text": content } }
                })
            })
            .collect();
        let har = json!({ "log": { "entries": entries } }).to_string();
        let (status, rows) = call(
            &app,
            "POST",
            &format!("/api/collections/{id}/import/har/preview"),
            Some(json!({ "har": har })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let text = rows.to_string();
        assert_eq!(rows.as_array().unwrap().len(), 500);
        assert!(text.len() < 1024 * 1024, "{}", text.len());
        assert!(!text.contains("MARKER-"));
    }
}

#[tokio::test]
async fn declare_secret_adds_name_to_environment_file() {
    let dir = small_collection("https://api.example.test");
    let app = app();
    let id = open(&app, dir.path()).await;
    let (status, body) = call(
        &app,
        "PUT",
        &format!("/api/collections/{id}/environments/local/secrets"),
        Some(json!({ "values": { "authorization": "Bearer tok-PASTE-1" }, "declare": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let text = std::fs::read_to_string(dir.path().join("environments/local.bru")).unwrap();
    assert!(text.contains("authorization"), "{text}");
    assert!(!text.contains("tok-PASTE-1"), "{text}");
    let (_, envs) = call(
        &app,
        "GET",
        &format!("/api/collections/{id}/environments"),
        None,
    )
    .await;
    assert_eq!(envs[0]["secretNames"], json!(["token", "authorization"]));
}
