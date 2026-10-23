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
    let response = &res["response"];
    assert_eq!(response["status"], 404);
    assert_eq!(response["bodyText"], "not here");
    assert_eq!(response["size"], 8);
    assert_eq!(response["truncated"], false);
    assert!(response["elapsedMs"].is_number());
    assert_eq!(res["tests"], json!([]));
    assert!(res["scriptError"].is_null());
}

#[tokio::test]
async fn send_runs_scripts_and_returns_tests() {
    let server = MockServer::start().await;
    Mock::given(path("/s"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "token": "abc" })))
        .mount(&server)
        .await;
    Mock::given(path("/t/abc"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let dir = small_collection(&server.uri());
    write(
        dir.path(),
        "s.bru",
        "meta {\n  name: s\n  type: http\n  seq: 9\n}\n\nget {\n  url: {{host}}/s\n}\n\nassert {\n  res.status: eq 200\n}\n\nscript:post-response {\n  bru.setVar('tok', res.body.token);\n  console.log('hi');\n}\n\ntests {\n  test('has token', () => expect(res.body.token).to.equal('abc'));\n}\n",
    );
    write(
        dir.path(),
        "t.bru",
        "meta {\n  name: t\n  type: http\n  seq: 10\n}\n\nget {\n  url: {{host}}/t/{{tok}}\n}\n",
    );
    let app = app();
    let id = open(&app, dir.path()).await;
    let send = |item: &str| json!({ "item": item, "env": "local" });
    let (status, res) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(send("s.bru")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(res["response"]["status"], 200);
    assert_eq!(
        res["tests"],
        json!([{ "name": "has token", "passed": true, "error": null }])
    );
    assert_eq!(res["asserts"][0]["lhs"], "res.status");
    assert_eq!(res["asserts"][0]["passed"], true);
    assert_eq!(res["console"], json!([{ "level": "log", "text": "hi" }]));
    // Runtime variables carry over to the next send in the session.
    let (status, res) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(send("t.bru")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(res["response"]["status"], 200);
}

#[tokio::test]
async fn send_reports_scripted_requests() {
    let server = MockServer::start().await;
    Mock::given(path("/main"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(path("/side"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "n": 1 })))
        .mount(&server)
        .await;
    let dir = small_collection(&server.uri());
    write(
        dir.path(),
        "m.bru",
        "meta {\n  name: m\n  type: http\n  seq: 9\n}\n\nget {\n  url: {{host}}/main\n}\n\nscript:post-response {\n  const r = await bru.sendRequest(bru.getEnvVar('host') + '/side');\n  bru.setVar('n', r.data.n);\n}\n",
    );
    let app = app();
    let id = open(&app, dir.path()).await;
    let (status, res) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(json!({ "item": "m.bru", "env": "local" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{res}");
    let scripted = &res["scriptedRequests"];
    assert_eq!(scripted.as_array().map(Vec::len), Some(1), "{res}");
    assert_eq!(scripted[0]["source"], "sendRequest");
    assert_eq!(scripted[0]["method"], "GET");
    assert_eq!(scripted[0]["status"], 200);
    assert!(scripted[0]["url"].as_str().unwrap().ends_with("/side"));
}

/// The Bruno app saves script changes to the active environment; secrets stay in memory.
#[tokio::test]
async fn send_persists_environment_changes_but_not_secrets() {
    let server = MockServer::start().await;
    Mock::given(path("/p"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = small_collection(&server.uri());
    write(
        dir.path(),
        "p.bru",
        "meta {\n  name: p\n  type: http\n  seq: 9\n}\n\nget {\n  url: {{host}}/p\n}\n\nscript:post-response {\n  bru.setEnvVar('saved', 'yes');\n  bru.setEnvVar('token', 'secret-value');\n}\n",
    );
    let app = app();
    let id = open(&app, dir.path()).await;
    let (status, res) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(json!({ "item": "p.bru", "env": "local" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{res}");
    let env = std::fs::read_to_string(dir.path().join("environments/local.bru")).unwrap();
    assert_eq!(
        env,
        format!(
            "vars {{\n  host: {}\n  saved: yes\n}}\nvars:secret [\n  token\n]\n",
            server.uri()
        )
    );
    // The environment list shows the saved value and the secret as set, never its value.
    let (_, envs) = call(
        &app,
        "GET",
        &format!("/api/collections/{id}/environments"),
        None,
    )
    .await;
    let local = &envs[0];
    assert!(local["vars"].to_string().contains("\"saved\""), "{envs}");
    assert!(!envs.to_string().contains("secret-value"), "{envs}");
}

#[tokio::test]
async fn send_pre_request_script_error_is_reported() {
    let dir = small_collection("http://127.0.0.1:9");
    write(
        dir.path(),
        "e.bru",
        "meta {\n  name: e\n  type: http\n  seq: 9\n}\n\nget {\n  url: {{host}}/e\n}\n\nscript:pre-request {\n  console.log('before');\n  throw new Error('nope');\n}\n",
    );
    let app = app();
    let id = open(&app, dir.path()).await;
    let (status, res) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(json!({ "item": "e.bru", "env": "local" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert!(res["response"].is_null());
    let e = &res["scriptError"];
    assert_eq!(e["phase"], "pre-request");
    assert_eq!(e["kind"], "thrown");
    assert_eq!(e["message"], "nope");
    assert_eq!(e["scope"], "request");
    assert_eq!(e["line"], 2);
    assert_eq!(res["console"][0]["text"], "before");
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
    const CAP: usize = 10 * 1024 * 1024;
    let server = MockServer::start().await;
    Mock::given(path("/a"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(vec![0xffu8; SIZE], "application/octet-stream"),
        )
        .mount(&server)
        .await;
    Mock::given(path("/b"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(vec![0xffu8; 10], "application/octet-stream"),
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
    let res = &res["response"];
    assert!(res["bodyText"].is_null());
    let b64 = res["bodyBase64"].as_str().unwrap();
    assert_eq!(b64.len(), CAP.div_ceil(3) * 4);
    assert_eq!(res["truncated"], true);
    // Reading stopped at the cap: the size is what was read, not the total.
    assert_eq!(res["sizeComplete"], false);
    let size = res["size"].as_u64().unwrap() as usize;
    assert!((CAP..=SIZE).contains(&size), "{size}");

    let (_, small) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(json!({ "item": "b.bru", "env": "local" })),
    )
    .await;
    let small = &small["response"];
    assert_eq!(small["truncated"], false);
    assert_eq!(small["sizeComplete"], true);
    assert_eq!(small["size"], 10);
}

#[tokio::test]
async fn truncation_backs_off_to_a_char_boundary() {
    const CAP: usize = 10 * 1024 * 1024;
    let mut body = vec![b'a'; CAP - 1];
    body.extend("é".repeat(1000).as_bytes());
    let server = MockServer::start().await;
    Mock::given(path("/a"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/plain; charset=utf-8"))
        .mount(&server)
        .await;
    let dir = small_collection(&server.uri());
    let app = app();
    let id = open(&app, dir.path()).await;
    let (_, res) = call(
        &app,
        "POST",
        &format!("/api/collections/{id}/send"),
        Some(json!({ "item": "a.bru", "env": "local" })),
    )
    .await;
    let res = &res["response"];
    assert!(res["bodyBase64"].is_null());
    assert_eq!(res["bodyText"].as_str().unwrap().len(), CAP - 1);
    assert_eq!(res["truncated"], true);
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
    assert_eq!(res["response"]["status"], 200);
    assert!(
        !fs::read_to_string(&env_path)
            .unwrap()
            .contains("s3cr3t-value")
    );

    // Later updates merge; null clears; only names are ever listed.
    let (status, _) = call(
        &app,
        "PUT",
        &format!("/api/collections/{id}/environments/local/secrets"),
        Some(json!({ "values": { "other": "x" } })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
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
    assert_eq!(local["secretsSet"], json!(["other", "token"]));
    assert!(!envs.to_string().contains("s3cr3t-value"));
    call(
        &app,
        "PUT",
        &format!("/api/collections/{id}/environments/local/secrets"),
        Some(json!({ "values": { "other": null } })),
    )
    .await;
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
    assert_eq!(local["secretsSet"], json!(["token"]));

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

#[tokio::test]
async fn extractor_rejections_are_json() {
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let dir = small_collection("http://127.0.0.1:9");
    let app = app();
    let id = open(&app, dir.path()).await;
    let cases = [
        (
            "POST",
            "/api/collections".to_owned(),
            Some("application/json"),
            "{not json",
            400,
            "bad_request",
        ),
        (
            "POST",
            "/api/collections".to_owned(),
            Some("text/plain"),
            "{}",
            415,
            "unsupported_media_type",
        ),
        (
            "POST",
            "/api/collections".to_owned(),
            Some("application/json"),
            "{}",
            422,
            "bad_request",
        ),
        (
            "GET",
            format!("/api/collections/{id}/requests"),
            None,
            "",
            400,
            "bad_request",
        ),
    ];
    for (method, uri, content_type, body, status, code) in cases {
        let mut req = Request::builder()
            .method(method)
            .uri(&uri)
            .header("host", format!("127.0.0.1:{PORT}"))
            .header("authorization", format!("Bearer {TOKEN}"));
        if let Some(ct) = content_type {
            req = req.header("content-type", ct);
        }
        let res = app
            .clone()
            .oneshot(req.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status().as_u16(), status, "{method} {uri} {body}");
        assert!(
            res.headers()["content-type"]
                .to_str()
                .unwrap()
                .starts_with("application/json")
        );
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"], code, "{json}");
        assert!(json["message"].as_str().is_some_and(|m| !m.is_empty()));
    }
}

#[tokio::test]
async fn save_returns_reread_request() {
    let (_dir, root) = fixture_copy();
    let app = app();
    let id = open(&app, &root).await;
    let uri = format!("/api/collections/{id}/requests?item={}", enc(ECHO_JSON));
    let (_, read) = call(&app, "GET", &uri, None).await;
    let mut request = read["request"].clone();
    request["headers"][0]["name"] = json!("~x");
    request["headers"][0]["value"] = json!("  v  ");
    let (status, saved) = call(
        &app,
        "PUT",
        &uri,
        Some(json!({ "request": request, "hash": read["hash"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        saved["request"]["headers"][0],
        json!({"name": "x", "value": "v", "enabled": false, "local": false})
    );
    let (_, again) = call(&app, "GET", &uri, None).await;
    assert_eq!(again["request"], saved["request"]);
    assert_eq!(again["hash"], saved["hash"]);
}

#[tokio::test]
async fn body_mode_switch_keeps_other_bodies() {
    let (_dir, root) = fixture_copy();
    let app = app();
    let id = open(&app, &root).await;
    let before = fs::read_to_string(root.join(ECHO_JSON)).unwrap();
    let uri = format!("/api/collections/{id}/requests?item={}", enc(ECHO_JSON));
    let (_, read) = call(&app, "GET", &uri, None).await;
    let json_body = read["request"]["body"].clone();
    assert_eq!(json_body["mode"], "json");
    assert_eq!(read["request"]["otherBodies"], json!([]));

    // json -> none: keeps body:json, changes only the mode line.
    let mut none = read["request"].clone();
    none["body"] = json!({ "mode": "none" });
    none["otherBodies"] = json!([json_body]);
    let (_, saved) = call(
        &app,
        "PUT",
        &uri,
        Some(json!({ "request": none, "hash": read["hash"] })),
    )
    .await;
    let after = fs::read_to_string(root.join(ECHO_JSON)).unwrap();
    assert_eq!(changed_lines(&before, &after), (1, 1), "{after}");
    assert_eq!(saved["request"]["otherBodies"], json!([json_body]));

    // none -> json restores the file exactly.
    let mut back = saved["request"].clone();
    back["body"] = json_body;
    back["otherBodies"] = json!([]);
    let (status, _) = call(
        &app,
        "PUT",
        &uri,
        Some(json!({ "request": back, "hash": saved["hash"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fs::read_to_string(root.join(ECHO_JSON)).unwrap(), before);
}
