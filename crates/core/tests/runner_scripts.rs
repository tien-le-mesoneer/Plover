//! Scripts, tests and asserts inside a run, in Bruno CLI order
//! (bruno-cli/src/runner/run-single-request.js, commands/run.js).

use std::fs;
use std::path::Path;

use plover_core::collection::Collection;
use plover_core::executor::{ExecOptions, Session};
use plover_core::runner::{RunEvent, RunOptions, RunReport, run, run_with};
use plover_core::script::ScriptError;
use plover_core::vars::ValueMap;
use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn setup(server: &MockServer, flow: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let scripts = flow
        .map(|f| format!(r#","scripts":{{"flow":"{f}"}}"#))
        .unwrap_or_default();
    write(
        dir.path(),
        "bruno.json",
        &format!(r#"{{"version":"1","name":"chain","type":"collection"{scripts}}}"#),
    );
    write(
        dir.path(),
        "environments/local.bru",
        &format!("vars {{\n  host: {}\n}}\n", server.uri()),
    );
    dir
}

/// A GET request file; `blocks` are appended verbatim.
fn request(name: &str, seq: u32, url_path: &str, blocks: &str) -> String {
    format!(
        "meta {{\n  name: {name}\n  type: http\n  seq: {seq}\n}}\n\nget {{\n  url: {{{{host}}}}{url_path}\n  body: none\n  auth: none\n}}\n{blocks}"
    )
}

fn text_block(name: &str, body: &str) -> String {
    let indented: Vec<String> = body.lines().map(|l| format!("  {l}")).collect();
    format!("\n{name} {{\n{}\n}}\n", indented.join("\n"))
}

async fn run_dir(root: &Path) -> (RunReport, Vec<RunEvent>) {
    let c = Collection::load(root).unwrap();
    let mut events = Vec::new();
    let opts = RunOptions {
        env: Some("local".into()),
        ..RunOptions::default()
    };
    let report = run(&c, root, &opts, &mut |e| {
        if !is_iteration_event(e) {
            events.push(e.clone())
        }
    })
    .await
    .unwrap();
    (report, events)
}

fn started(events: &[RunEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            RunEvent::Started { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn chained_login_sets_token() {
    let server = MockServer::start().await;
    Mock::given(path("/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "token": "t-123" })))
        .mount(&server)
        .await;
    Mock::given(path("/me"))
        .and(header("authorization", "Bearer t-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "name": "Ana" })))
        .mount(&server)
        .await;
    Mock::given(path("/me"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let dir = setup(&server, None);
    let root = dir.path();
    write(
        root,
        "login.bru",
        &request(
            "login",
            1,
            "/login",
            &text_block(
                "script:post-response",
                "bru.setVar('token', res.body.token);",
            ),
        ),
    );
    let me = "meta {\n  name: me\n  type: http\n  seq: 2\n}\n\nget {\n  url: {{host}}/me\n  body: none\n  auth: bearer\n}\n\nauth:bearer {\n  token: {{token}}\n}\n\nassert {\n  res.status: eq 200\n  res.body.name: isString\n}\n";
    let me = format!(
        "{me}{}",
        text_block(
            "tests",
            "test('is Ana', () => expect(res.body.name).to.equal('Ana'));"
        )
    );
    write(root, "me.bru", &me);
    let (report, events) = run_dir(root).await;
    assert_eq!(
        (report.passed, report.failed, report.skipped),
        (2, 0, 0),
        "{events:#?}"
    );
    assert_eq!((report.tests_passed, report.tests_failed), (1, 0));
    assert_eq!((report.asserts_passed, report.asserts_failed), (2, 0));
    let me_event = events
        .iter()
        .find_map(|e| match e {
            RunEvent::Passed { outcome, .. }
                if outcome.status == Some(200) && !outcome.tests.is_empty() =>
            {
                Some(outcome.clone())
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(me_event.tests[0].name, "is Ana");
}

/// Each scope appends its letter to runtime var `o`; folder `f`, request `r`, collection `c`.
async fn flow_order(flow: Option<&str>) -> String {
    let server = MockServer::start().await;
    Mock::given(path("/x"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = setup(&server, flow);
    let root = dir.path();
    let append = |l: &str| format!("bru.setVar('o', (bru.getVar('o') || '') + '{l},');");
    write(
        root,
        "collection.bru",
        &format!(
            "{}{}",
            text_block("script:pre-request", &append("c")),
            text_block("script:post-response", &append("c"))
        ),
    );
    write(
        root,
        "f/folder.bru",
        &format!(
            "meta {{\n  name: f\n}}\n{}{}",
            text_block("script:pre-request", &append("f")),
            text_block("script:post-response", &append("f"))
        ),
    );
    write(
        root,
        "f/r.bru",
        &request(
            "r",
            1,
            "/x",
            &format!(
                "{}{}",
                text_block("script:pre-request", &append("r")),
                text_block("script:post-response", &append("r"))
            ),
        ),
    );
    let c = Collection::load(root).unwrap();
    let session = Session::new(ExecOptions::default()).unwrap();
    let mut runtime = ValueMap::new();
    let opts = RunOptions {
        env: Some("local".into()),
        ..RunOptions::default()
    };
    let report = run_with(&c, root, &opts, &session, &mut runtime, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(report.passed, 1);
    runtime["o"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn sandwich_order() {
    assert_eq!(flow_order(None).await, "c,f,r,r,f,c,");
}

#[tokio::test]
async fn sequential_order() {
    assert_eq!(flow_order(Some("sequential")).await, "c,f,r,c,f,r,");
}

#[tokio::test]
async fn pre_request_sees_raw_request_and_mutations_are_sent() {
    let server = MockServer::start().await;
    Mock::given(path("/x"))
        .and(query_param("q", "1"))
        .and(header("x-pre", "{{host}}/x"))
        .and(header("x-id", "42"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = setup(&server, None);
    let root = dir.path();
    write(
        root,
        "r.bru",
        &request(
            "r",
            1,
            "/x",
            &format!(
                "\nheaders {{\n  x-id: {{{{id}}}}\n}}\n{}",
                text_block(
                    "script:pre-request",
                    "req.setHeader('x-pre', req.getUrl());\nreq.setUrl(req.getUrl() + '?q=1');\nbru.setVar('id', 42);"
                )
            ),
        ),
    );
    let (report, events) = run_dir(root).await;
    assert_eq!(report.passed, 1, "{events:#?}");
    match &events[0] {
        RunEvent::Started { url, .. } => assert!(url.ends_with("/x?q=1"), "{url}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn skip_request() {
    let server = MockServer::start().await;
    Mock::given(path("/x"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let dir = setup(&server, None);
    write(
        dir.path(),
        "r.bru",
        &request(
            "r",
            1,
            "/x",
            &text_block("script:pre-request", "bru.runner.skipRequest();"),
        ),
    );
    let (report, events) = run_dir(dir.path()).await;
    assert_eq!((report.passed, report.skipped), (0, 1));
    assert!(
        matches!(&events[0], RunEvent::Skipped { reason, .. } if reason == "request skipped via pre-request script"),
        "{events:?}"
    );
}

#[tokio::test]
async fn stop_execution_skips_the_rest() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = setup(&server, None);
    let root = dir.path();
    write(
        root,
        "a.bru",
        &request(
            "a",
            1,
            "/a",
            &text_block("script:post-response", "bru.runner.stopExecution();"),
        ),
    );
    write(root, "b.bru", &request("b", 2, "/b", ""));
    write(root, "c.bru", &request("c", 3, "/c", ""));
    let (report, events) = run_dir(root).await;
    assert_eq!(started(&events), ["a"]);
    assert_eq!((report.passed, report.skipped), (1, 2));
}

#[tokio::test]
async fn set_next_request_jumps() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = setup(&server, None);
    let root = dir.path();
    write(
        root,
        "a.bru",
        &request(
            "a",
            1,
            "/a",
            &text_block("script:post-response", "bru.setNextRequest('c');"),
        ),
    );
    write(root, "b.bru", &request("b", 2, "/b", ""));
    write(
        root,
        "c.bru",
        &request(
            "c",
            3,
            "/c",
            &text_block("tests", "bru.runner.setNextRequest(null);"),
        ),
    );
    write(root, "d.bru", &request("d", 4, "/d", ""));
    let (_, events) = run_dir(root).await;
    assert_eq!(started(&events), ["a", "c"]);
}

#[tokio::test]
async fn error_reports_scope_and_line() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = setup(&server, None);
    let root = dir.path();
    write(
        root,
        "users/folder.bru",
        &format!(
            "meta {{\n  name: users\n}}\n{}",
            text_block(
                "script:post-response",
                "bru.setVar('kept', 1);\n\nthrow new Error('folder broke');"
            )
        ),
    );
    write(root, "users/a.bru", &request("a", 1, "/a", ""));
    write(root, "z.bru", &request("z", 9, "/z", ""));
    let c = Collection::load(root).unwrap();
    let session = Session::new(ExecOptions::default()).unwrap();
    let mut runtime = ValueMap::new();
    let mut events = Vec::new();
    let opts = RunOptions {
        env: Some("local".into()),
        ..RunOptions::default()
    };
    let report = run_with(&c, root, &opts, &session, &mut runtime, &mut |e| {
        if !is_iteration_event(e) {
            events.push(e.clone())
        }
    })
    .await
    .unwrap();
    assert_eq!((report.passed, report.failed), (1, 1), "{events:#?}");
    let failure = events
        .iter()
        .find_map(|e| match e {
            RunEvent::Failed { outcome, .. } => outcome.script_error.clone(),
            _ => None,
        })
        .expect("a script failure");
    assert_eq!(failure.phase.as_str(), "post-response");
    match failure.error {
        ScriptError::Thrown {
            message,
            scope,
            line,
            ..
        } => {
            assert_eq!(message, "folder broke");
            assert_eq!(scope.as_deref(), Some("folder users"));
            assert_eq!(line, Some(3));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(runtime["kept"], json!(1));
}

#[tokio::test]
async fn failed_test_or_assert_fails_the_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = setup(&server, None);
    let root = dir.path();
    write(
        root,
        "a.bru",
        &request(
            "a",
            1,
            "/a",
            &text_block("tests", "test('no', () => expect(1).to.equal(2));"),
        ),
    );
    write(
        root,
        "b.bru",
        &request("b", 2, "/b", "\nassert {\n  res.status: eq 201\n}\n"),
    );
    let (report, _) = run_dir(root).await;
    assert_eq!((report.passed, report.failed), (0, 2));
    assert_eq!((report.tests_passed, report.tests_failed), (0, 1));
    assert_eq!((report.asserts_passed, report.asserts_failed), (0, 1));
}

#[tokio::test]
async fn require_fs_fails_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let dir = setup(&server, None);
    write(
        dir.path(),
        "a.bru",
        &request(
            "a",
            1,
            "/a",
            &text_block("script:pre-request", "const fs = require('fs');"),
        ),
    );
    let (report, events) = run_dir(dir.path()).await;
    assert_eq!(report.failed, 1);
    match &events[0] {
        RunEvent::Failed { error, .. } => {
            assert!(error.contains("Cannot find module 'fs'"), "{error}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn post_response_vars_and_test_script_errors() {
    let server = MockServer::start().await;
    Mock::given(path("/a"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 5 })))
        .mount(&server)
        .await;
    Mock::given(path("/b/5"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = setup(&server, None);
    let root = dir.path();
    write(
        root,
        "a.bru",
        &request(
            "a",
            1,
            "/a",
            "\nvars:post-response {\n  id: res.body.id\n}\n",
        ),
    );
    write(
        root,
        "b.bru",
        &request(
            "b",
            2,
            "/b/{{id}}",
            &text_block("tests", "test('ok', () => {});\nthrow new Error('late');"),
        ),
    );
    let (report, events) = run_dir(root).await;
    // b's test script throws: Bruno records it as a failed "Test Script Error" test.
    assert_eq!((report.passed, report.failed), (1, 1), "{events:#?}");
    let b = events
        .iter()
        .find_map(|e| match e {
            RunEvent::Failed { outcome, .. } => Some(outcome.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(b.status, Some(200));
    let names: Vec<&str> = b.tests.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["ok", "Test Script Error"]);
}

#[tokio::test]
async fn object_variable_paths_in_urls() {
    let server = MockServer::start().await;
    Mock::given(path("/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "user": { "id": 42 } })))
        .mount(&server)
        .await;
    Mock::given(path("/users/42"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let dir = setup(&server, None);
    let root = dir.path();
    write(
        root,
        "a.bru",
        &request(
            "a",
            1,
            "/login",
            &text_block("script:post-response", "bru.setVar('user', res.body.user);"),
        ),
    );
    write(root, "b.bru", &request("b", 2, "/users/{{user.id}}", ""));
    let (report, events) = run_dir(root).await;
    assert_eq!(report.passed, 2, "{events:#?}");
}

#[tokio::test]
async fn deep_variable_chain_in_url_does_not_abort() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = setup(&server, None);
    let root = dir.path();
    write(
        root,
        "a.bru",
        &request(
            "a",
            1,
            "/{{a0}}",
            &text_block(
                "script:pre-request",
                "const N = 20000; for (let i = 0; i < N; i++) bru.setVar('a' + i, '{{a' + (i + 1) + '}}'); bru.setVar('a' + N, 'x');",
            ),
        ),
    );
    write(root, "b.bru", &request("b", 2, "/b", ""));
    let (report, events) = run_dir(root).await;
    assert_eq!(report.passed + report.failed, 2, "{events:#?}");
    assert_eq!(started(&events), ["a", "b"]);
}

/// Request events only; the iteration events of a plain run are tested in `data_runs.rs`.
fn is_iteration_event(e: &RunEvent) -> bool {
    matches!(
        e,
        RunEvent::IterationStarted { .. } | RunEvent::IterationFinished { .. }
    )
}
