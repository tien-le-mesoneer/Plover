use std::fs;
use std::path::Path;

use brs_core::collection::Collection;
use brs_core::runner::{RunError, RunEvent, RunOptions, run};
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
) -> (Result<brs_core::runner::RunReport, RunError>, Vec<RunEvent>) {
    let c = Collection::load(root).unwrap();
    let mut events = Vec::new();
    let report = run(&c, root, opts, &mut |e| events.push(e.clone())).await;
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
        RunEvent::Skipped { path, reason } => Some((path, reason)),
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
    use brs_core::executor::{ExecOptions, Session};
    use brs_core::runner::run_with;
    use brs_core::vars::ValueMap;
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
    let login = brs_core::resolve::ResolvedRequest {
        method: brs_core::request::Method::Get,
        url: format!("{}/login", server.uri()),
        headers: Vec::new(),
        body: brs_core::resolve::ResolvedBody::None,
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
