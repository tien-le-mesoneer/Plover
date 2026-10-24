use std::fs;
use std::path::Path;

use plover_core::collection::Collection;
use plover_core::runner::{RunError, RunEvent, RunOptions, run};
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn request(name: &str, seq: u32) -> String {
    format!(
        "meta {{\n  name: {name}\n  type: http\n  seq: {seq}\n}}\n\nget {{\n  url: {{{{host}}}}/{name}\n}}\n"
    )
}

fn collection(root: &Path) {
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"t","type":"collection"}"#,
    );
}

async fn run_all(
    root: &Path,
    opts: &RunOptions,
) -> (
    Result<plover_core::runner::RunReport, RunError>,
    Vec<RunEvent>,
) {
    let c = Collection::load(root).unwrap();
    let mut events = Vec::new();
    let report = run(&c, root, opts, &mut |e| {
        if !is_iteration_event(e) {
            events.push(e.clone())
        }
    })
    .await;
    (report, events)
}

fn env_opts() -> RunOptions {
    RunOptions {
        env: Some("local".into()),
        ..RunOptions::default()
    }
}

#[tokio::test]
async fn runs_in_order_and_reports() {
    let server = MockServer::start().await;
    Mock::given(path("/z"))
        .respond_with(ResponseTemplate::new(200).set_body_string("zz"))
        .mount(&server)
        .await;
    Mock::given(path("/a"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(path("/m"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    collection(root);
    write(
        root,
        "environments/local.bru",
        &format!("vars {{\n  host: {}\n}}\n", server.uri()),
    );
    // Seq order (z, a, m) differs from name order, so this fails if seq is ignored.
    write(root, "m.bru", &request("m", 3));
    write(root, "z.bru", &request("z", 1));
    write(root, "a.bru", &request("a", 2));
    let (report, events) = run_all(root, &env_opts()).await;
    let report = report.unwrap();
    assert_eq!((report.passed, report.failed, report.skipped), (3, 0, 0));
    let started: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            RunEvent::Started { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(started, ["z", "a", "m"]);
    assert!(matches!(
        &events[1],
        RunEvent::Passed { outcome, .. } if outcome.status == Some(200) && outcome.size == 2
    ));
    assert!(
        matches!(&events[5], RunEvent::Passed { outcome, .. } if outcome.status == Some(500)),
        "5xx is not a failure"
    );
}

#[tokio::test]
async fn broken_file_skipped_run_continues() {
    let server = MockServer::start().await;
    Mock::given(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    collection(root);
    write(
        root,
        "environments/local.bru",
        &format!("vars {{\n  host: {}\n}}\n", server.uri()),
    );
    write(root, "broken.bru", "get {\n");
    write(root, "ok.bru", &request("ok", 1));
    let (report, events) = run_all(root, &env_opts()).await;
    let report = report.unwrap();
    assert_eq!((report.passed, report.failed, report.skipped), (1, 0, 1));
    let skipped = events.iter().find_map(|e| match e {
        RunEvent::Skipped { path, reason, .. } => Some((path, reason)),
        _ => None,
    });
    let (path, reason) = skipped.unwrap();
    assert!(path.ends_with("broken.bru"));
    assert!(reason.starts_with("parse error:"), "{reason}");
}

#[tokio::test]
async fn graphql_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    collection(root);
    write(
        root,
        "g.bru",
        "meta {\n  name: g\n  type: graphql\n}\n\npost {\n  url: http://h\n  body: graphql\n}\n",
    );
    let (report, events) = run_all(root, &RunOptions::default()).await;
    assert_eq!(report.unwrap().skipped, 1);
    assert!(matches!(&events[0], RunEvent::Skipped { reason, .. } if reason.contains("graphql")));
}

#[tokio::test]
async fn transport_error_fails_and_setup_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    collection(root);
    write(
        root,
        "environments/local.bru",
        "vars {\n  host: http://127.0.0.1:9\n}\n",
    );
    write(root, "a.bru", &request("a", 1));
    let (report, _) = run_all(root, &env_opts()).await;
    assert_eq!(report.unwrap().failed, 1);
    let unknown = RunOptions {
        env: Some("nope".into()),
        ..RunOptions::default()
    };
    assert!(matches!(
        run_all(root, &unknown).await.0,
        Err(RunError::UnknownEnvironment(_))
    ));
    let c = Collection::load(root).unwrap();
    let missing = run(
        &c,
        &root.join("missing.bru"),
        &RunOptions::default(),
        &mut |_| {},
    )
    .await;
    assert!(matches!(missing, Err(RunError::TargetNotFound(_))));
}

#[tokio::test]
async fn finished_is_last_and_carries_report() {
    let server = MockServer::start().await;
    Mock::given(path("/a"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    collection(root);
    write(
        root,
        "environments/local.bru",
        &format!("vars {{\n  host: {}\n}}\n", server.uri()),
    );
    write(root, "a.bru", &request("a", 1));
    let (report, events) = run_all(root, &env_opts()).await;
    let report = report.unwrap();
    assert_eq!(events.last(), Some(&RunEvent::Finished { report }));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, RunEvent::Finished { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn run_with_shares_session_cookies_and_runtime() {
    use plover_core::executor::{ExecOptions, Session};
    use plover_core::runner::run_with;
    use plover_core::vars::ValueMap;
    use wiremock::matchers::header;

    let server = MockServer::start().await;
    Mock::given(path("/a"))
        .and(header("cookie", "sid=1"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    collection(root);
    write(root, "a.bru", &request("a", 1));
    let c = Collection::load(root).unwrap();
    let session = Session::new(ExecOptions::default()).unwrap();
    // Seed the session's jar through a prior response.
    Mock::given(path("/login"))
        .respond_with(ResponseTemplate::new(200).insert_header("set-cookie", "sid=1; Path=/"))
        .mount(&server)
        .await;
    let login = plover_core::resolve::ResolvedRequest {
        method: plover_core::request::Method::Get,
        url: format!("{}/login", server.uri()),
        headers: Vec::new(),
        body: plover_core::resolve::ResolvedBody::None,
        settings: Default::default(),
    };
    session.send(&login).await.unwrap();
    let mut runtime = ValueMap::from([("host".to_owned(), server.uri().into())]);
    let report = run_with(
        &c,
        root,
        &RunOptions::default(),
        &session,
        &mut runtime,
        &mut |_| {},
    )
    .await
    .unwrap();
    assert_eq!(report.passed, 1);
}

#[tokio::test]
async fn delay_waits_between_requests_not_after_the_last() {
    // Bruno commands/run.js: after every request except the last.
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    collection(root);
    write(
        root,
        "environments/local.bru",
        &format!("vars {{\n  host: {}\n}}\n", server.uri()),
    );
    for (i, n) in ["a", "b", "c"].iter().enumerate() {
        write(root, &format!("{n}.bru"), &request(n, i as u32 + 1));
    }
    let opts = RunOptions {
        delay: Some(std::time::Duration::from_millis(300)),
        ..env_opts()
    };
    let started = std::time::Instant::now();
    let (report, _) = run_all(root, &opts).await;
    let elapsed = started.elapsed();
    assert_eq!(report.unwrap().passed, 3);
    assert!(
        elapsed >= std::time::Duration::from_millis(600),
        "{elapsed:?}"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(890),
        "no delay after the last: {elapsed:?}"
    );

    // A single request never waits.
    let one = tempfile::tempdir().unwrap();
    collection(one.path());
    write(
        one.path(),
        "environments/local.bru",
        &format!("vars {{\n  host: {}\n}}\n", server.uri()),
    );
    write(one.path(), "a.bru", &request("a", 1));
    let started = std::time::Instant::now();
    run_all(one.path(), &opts).await.0.unwrap();
    assert!(started.elapsed() < std::time::Duration::from_millis(290));
}

mod detail {
    use super::*;
    use plover_core::run_record::{BodyDetail, MAX_DETAIL_BODY, PartDetail};
    use plover_core::runner::RequestOutcome;

    fn env(root: &Path, host: &str) {
        write(
            root,
            "environments/local.bru",
            &format!("vars {{\n  host: {host}\n  x: 1\n}}\n"),
        );
    }

    /// The outcome of the only request of the run.
    async fn only_outcome(root: &Path) -> RequestOutcome {
        let (_, events) = run_all(root, &env_opts()).await;
        events
            .into_iter()
            .find_map(|e| match e {
                RunEvent::Passed { outcome, .. } | RunEvent::Failed { outcome, .. } => {
                    Some(outcome)
                }
                _ => None,
            })
            .expect("a finished request")
    }

    #[tokio::test]
    async fn run_keeps_request_and_response_detail() {
        let server = MockServer::start().await;
        Mock::given(path("/p"))
            .respond_with(
                ResponseTemplate::new(201)
                    .insert_header("x-r", "y")
                    .set_body_string("ok"),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        collection(root);
        env(root, &server.uri());
        write(
            root,
            "p.bru",
            "meta {\n  name: p\n  type: http\n  seq: 1\n}\n\npost {\n  url: {{host}}/p\n  body: json\n  auth: none\n}\n\nbody:json {\n  {\"a\":\"{{x}}\"}\n}\n",
        );
        let outcome = only_outcome(root).await;
        let request = outcome.request.expect("request detail");
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, format!("{}/p", server.uri()));
        assert!(
            request
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        );
        assert!(
            matches!(&request.body, BodyDetail::Text { text, truncated: false, .. } if text == "{\"a\":\"1\"}"),
            "{:?}",
            request.body
        );
        let response = outcome.response.expect("response detail");
        assert_eq!(response.status, 201);
        assert_eq!(response.status_text, "Created");
        assert!(
            response
                .headers
                .contains(&("x-r".to_owned(), "y".to_owned()))
        );
        assert_eq!(
            response.body,
            BodyDetail::Text {
                text: "ok".into(),
                size: 2,
                truncated: false
            }
        );
    }

    #[tokio::test]
    async fn set_body_is_what_detail_shows() {
        let server = MockServer::start().await;
        Mock::given(path("/a"))
            .respond_with(ResponseTemplate::new(200).set_body_string("orig"))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        collection(root);
        env(root, &server.uri());
        write(
            root,
            "a.bru",
            &format!(
                "{}\nscript:post-response {{\n  res.setBody({{b: 2}});\n}}\n",
                request("a", 1)
            ),
        );
        let outcome = only_outcome(root).await;
        assert!(
            matches!(&outcome.response.unwrap().body, BodyDetail::Text { text, .. } if text == "{\"b\":2}")
        );
    }

    #[tokio::test]
    async fn transport_error_keeps_request_detail() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        collection(root);
        env(root, "http://127.0.0.1:9");
        write(root, "a.bru", &request("a", 1));
        let outcome = only_outcome(root).await;
        assert!(outcome.request.is_some());
        assert!(outcome.response.is_none());
    }

    #[tokio::test]
    async fn pre_request_error_has_no_detail() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        collection(root);
        env(root, "http://127.0.0.1:9");
        write(
            root,
            "a.bru",
            &format!(
                "{}\nscript:pre-request {{\n  throw new Error(\"x\");\n}}\n",
                request("a", 1)
            ),
        );
        let outcome = only_outcome(root).await;
        assert!(outcome.request.is_none());
        assert!(outcome.response.is_none());
    }

    #[tokio::test]
    async fn binary_response_is_base64() {
        let server = MockServer::start().await;
        Mock::given(path("/a"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0xff, 0x00, 0x01]))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        collection(root);
        env(root, &server.uri());
        write(root, "a.bru", &request("a", 1));
        let outcome = only_outcome(root).await;
        assert_eq!(
            outcome.response.unwrap().body,
            BodyDetail::Base64 {
                data: "/wAB".into(),
                size: 3,
                truncated: false
            }
        );
    }

    #[tokio::test]
    async fn large_body_is_capped() {
        let server = MockServer::start().await;
        Mock::given(path("/a"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("a".repeat(MAX_DETAIL_BODY + 10)),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        collection(root);
        env(root, &server.uri());
        write(root, "a.bru", &request("a", 1));
        let outcome = only_outcome(root).await;
        match outcome.response.unwrap().body {
            BodyDetail::Text {
                text,
                size,
                truncated,
            } => {
                assert_eq!(size, MAX_DETAIL_BODY + 10);
                assert!(truncated);
                assert_eq!(text.len(), MAX_DETAIL_BODY);
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn multipart_file_part_keeps_only_name() {
        let server = MockServer::start().await;
        Mock::given(path("/a"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        collection(root);
        env(root, &server.uri());
        write(root, "files/upload.txt", "FILE-BYTES-0123");
        write(
            root,
            "a.bru",
            "meta {\n  name: a\n  type: http\n  seq: 1\n}\n\npost {\n  url: {{host}}/a\n  body: multipartForm\n  auth: none\n}\n\nbody:multipart-form {\n  note: hi\n  doc: @file(files/upload.txt)\n}\n",
        );
        let outcome = only_outcome(root).await;
        let body = outcome.request.unwrap().body;
        let BodyDetail::Multipart { parts } = &body else {
            panic!("{body:?}");
        };
        assert!(
            parts.iter().any(
                |p| matches!(p, PartDetail::File { file_name, .. } if file_name == "upload.txt")
            )
        );
        assert!(
            parts
                .iter()
                .any(|p| matches!(p, PartDetail::Text { value, .. } if value == "hi"))
        );
        assert!(!format!("{body:?}").contains("FILE-BYTES"));
    }
}

// ---- scratch sends (no file behind the request)

mod scratch {
    use super::*;
    use plover_core::executor::ExecOptions;
    use plover_core::request::{HttpRequest, KeyValue};
    use plover_core::runner::{SendError, SendInput, send_one};
    use plover_core::workspace::Workspace;
    use wiremock::matchers::header;

    fn scoped_collection(root: &Path, server: &str) {
        collection(root);
        write(
            root,
            "collection.bru",
            "headers {\n  x-col: 1\n}\n\nscript:pre-request {\n  req.setHeader(\"x-pre\", \"yes\");\n}\n",
        );
        write(root, "f/folder.bru", "headers {\n  x-folder: 1\n}\n");
        write(root, "f/r.bru", &request("r", 1));
        write(
            root,
            "environments/local.bru",
            &format!("vars {{\n  host: {server}\n}}\n"),
        );
    }

    fn draft(url: &str) -> HttpRequest {
        let mut req = HttpRequest::blank();
        req.url = url.to_owned();
        req.headers = vec![KeyValue::new("x-own", "1")];
        req
    }

    #[tokio::test]
    async fn scratch_send_applies_collection_headers_not_folder_headers() {
        let server = MockServer::start().await;
        Mock::given(path("/scratch"))
            .and(header("x-col", "1"))
            .and(header("x-own", "1"))
            .and(header("x-pre", "yes"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        scoped_collection(dir.path(), &server.uri());
        let ws = Workspace::new(ExecOptions::default());
        let open = ws.get(&ws.open(dir.path()).unwrap()).unwrap();
        let req = draft("{{host}}/scratch");
        let sent = send_one(
            &open,
            SendInput {
                item_id: None,
                draft: Some(&req),
                env: Some("local"),
            },
        )
        .await
        .unwrap();
        assert_eq!(sent.response.unwrap().status, 200);
        let received = server.received_requests().await.unwrap();
        assert!(received[0].headers.get("x-folder").is_none());
        // Nothing was written for the scratch request.
        assert!(!dir.path().join(".plover-scratch.bru").exists());
    }

    #[tokio::test]
    async fn scratch_send_without_draft_is_error() {
        let dir = tempfile::tempdir().unwrap();
        scoped_collection(dir.path(), "http://127.0.0.1:9");
        let ws = Workspace::new(ExecOptions::default());
        let open = ws.get(&ws.open(dir.path()).unwrap()).unwrap();
        let err = send_one(
            &open,
            SendInput {
                item_id: None,
                draft: None,
                env: None,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, SendError::NotFound(_)), "{err:?}");
    }
}

/// Request events only; the iteration events of a plain run are tested in `data_runs.rs`.
fn is_iteration_event(e: &RunEvent) -> bool {
    matches!(
        e,
        RunEvent::IterationStarted { .. } | RunEvent::IterationFinished { .. }
    )
}
