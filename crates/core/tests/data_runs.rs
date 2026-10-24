//! Data-driven runs: one pass over the requests per data-file row or `--iteration-count`
//! (Bruno CLI 4.3.0 commands/run.js 1200-1560).

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use plover_core::collection::Collection;
use plover_core::runner::{Iterations, RunEvent, RunOptions, RunReport, run};
use plover_core::vars::ValueMap;
use serde_json::{Value, json};
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn row(v: Value) -> ValueMap {
    v.as_object().unwrap().clone().into_iter().collect()
}

fn setup(server: &MockServer) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "bruno.json",
        r#"{"version":"1","name":"data","type":"collection"}"#,
    );
    write(
        dir.path(),
        "environments/local.bru",
        &format!("vars {{\n  host: {}\n}}\n", server.uri()),
    );
    dir
}

fn request(name: &str, seq: u32, url_path: &str, blocks: &str) -> String {
    format!(
        "meta {{\n  name: {name}\n  type: http\n  seq: {seq}\n}}\n\nget {{\n  url: {{{{host}}}}{url_path}\n  body: none\n  auth: none\n}}\n{blocks}"
    )
}

fn block(name: &str, body: &str) -> String {
    let indented: Vec<String> = body.lines().map(|l| format!("  {l}")).collect();
    format!("\n{name} {{\n{}\n}}\n", indented.join("\n"))
}

async fn mock_ok(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path_regex(".*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(server)
        .await;
}

async fn run_with(root: &Path, iterations: Iterations) -> (RunReport, Vec<RunEvent>) {
    run_opts(
        root,
        RunOptions {
            env: Some("local".into()),
            iterations,
            ..RunOptions::default()
        },
    )
    .await
}

async fn run_opts(root: &Path, opts: RunOptions) -> (RunReport, Vec<RunEvent>) {
    let c = Collection::load(root).unwrap();
    let mut events = Vec::new();
    let report = run(&c, root, &opts, &mut |e| events.push(e.clone()))
        .await
        .unwrap();
    (report, events)
}

async fn paths(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_owned())
        .collect()
}

/// Compact trace: `I0/2`, `S0:a`, `P0`, `F0`, `K0:reason`, `E0`, `D`.
fn trace(events: &[RunEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| match e {
            RunEvent::IterationStarted { index, total, .. } => format!("I{index}/{total}"),
            RunEvent::IterationFinished { index, .. } => format!("E{index}"),
            RunEvent::Started {
                iteration, name, ..
            } => format!("S{iteration}:{name}"),
            RunEvent::Passed { iteration, .. } => format!("P{iteration}"),
            RunEvent::Failed { iteration, .. } => format!("F{iteration}"),
            RunEvent::Skipped {
                iteration, reason, ..
            } => format!("K{iteration}:{reason}"),
            RunEvent::Finished { .. } => "D".into(),
        })
        .collect()
}

#[tokio::test]
async fn plain_run_is_one_iteration() {
    let server = MockServer::start().await;
    mock_ok(&server).await;
    let dir = setup(&server);
    write(dir.path(), "a.bru", &request("a", 1, "/a", ""));
    let (report, events) = run_with(dir.path(), Iterations::default()).await;
    assert_eq!(report.iterations, 1);
    assert_eq!(trace(&events), ["I0/1", "S0:a", "P0", "E0", "D"]);
    assert!(matches!(&events[0], RunEvent::IterationStarted { data, .. } if data.is_empty()));
}

#[tokio::test]
async fn runs_once_per_row() {
    let server = MockServer::start().await;
    mock_ok(&server).await;
    let dir = setup(&server);
    write(dir.path(), "a.bru", &request("a", 1, "/x/{{id}}", ""));
    write(dir.path(), "b.bru", &request("b", 2, "/b", ""));
    let rows = vec![row(json!({"id":"1"})), row(json!({"id":"2"}))];
    let (report, events) = run_with(dir.path(), Iterations::from_rows(rows)).await;
    assert_eq!(paths(&server).await, ["/x/1", "/b", "/x/2", "/b"]);
    assert_eq!(
        trace(&events),
        [
            "I0/2", "S0:a", "P0", "S0:b", "P0", "E0", "I1/2", "S1:a", "P1", "S1:b", "P1", "E1", "D"
        ]
    );
    assert_eq!(report.iterations, 2);
    assert_eq!(report.passed, 4);
    assert!(
        matches!(&events[6], RunEvent::IterationStarted { data, .. } if data["id"] == json!("2"))
    );
}

#[tokio::test]
async fn repeat_without_file() {
    let server = MockServer::start().await;
    mock_ok(&server).await;
    let dir = setup(&server);
    write(dir.path(), "a.bru", &request("a", 1, "/a", ""));
    let (report, events) = run_with(dir.path(), Iterations::repeat(3)).await;
    assert_eq!(paths(&server).await.len(), 3);
    assert_eq!(report.iterations, 3);
    assert!(events.iter().all(|e| !matches!(e,
        RunEvent::IterationStarted { data, .. } if !data.is_empty())));
}

#[tokio::test]
async fn setvar_carries_over() {
    let server = MockServer::start().await;
    mock_ok(&server).await;
    let dir = setup(&server);
    write(
        dir.path(),
        "a.bru",
        &request(
            "a",
            1,
            "/a",
            &block(
                "script:post-response",
                r#"bru.setVar("n", (bru.getVar("n") || 0) + 1);"#,
            ),
        ),
    );
    write(dir.path(), "b.bru", &request("b", 2, "/b/{{n}}", ""));
    run_with(dir.path(), Iterations::repeat(2)).await;
    assert_eq!(paths(&server).await, ["/a", "/b/1", "/a", "/b/2"]);
}

#[tokio::test]
async fn iteration_data_set_is_per_iteration() {
    let server = MockServer::start().await;
    mock_ok(&server).await;
    let dir = setup(&server);
    write(
        dir.path(),
        "a.bru",
        &request(
            "a",
            1,
            "/a/{{k}}",
            &block(
                "script:pre-request",
                r#"bru.setVar("seen" + bru.runner.iterationIndex, bru.runner.iterationData.has("k"));
bru.runner.iterationData.set("k", "v" + bru.runner.iterationIndex);"#,
            ),
        ),
    );
    write(
        dir.path(),
        "b.bru",
        &request("b", 2, "/b/{{k}}/{{seen0}}/{{seen1}}", ""),
    );
    let rows = vec![row(json!({"u":"x"})), row(json!({"u":"y"}))];
    let (_, events) = run_with(dir.path(), Iterations::from_rows(rows)).await;
    // The request's own URL and the next request see the set; iteration 2 starts clean.
    assert_eq!(
        paths(&server).await,
        [
            "/a/v0",
            "/b/v0/false/%7B%7Bseen1%7D%7D",
            "/a/v1",
            "/b/v1/false/false"
        ]
    );
    // The final row (with the change) is reported when the iteration ends.
    assert!(events.iter().any(|e| matches!(e,
        RunEvent::IterationFinished { index: 1, data } if data["k"] == json!("v1") && data["u"] == json!("y"))));
}

#[tokio::test]
async fn delay_not_after_last_of_iteration() {
    let server = MockServer::start().await;
    mock_ok(&server).await;
    let dir = setup(&server);
    write(dir.path(), "a.bru", &request("a", 1, "/a", ""));
    write(dir.path(), "b.bru", &request("b", 2, "/b", ""));
    let started = Instant::now();
    run_opts(
        dir.path(),
        RunOptions {
            env: Some("local".into()),
            iterations: Iterations::repeat(2),
            delay: Some(Duration::from_millis(200)),
            ..RunOptions::default()
        },
    )
    .await;
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(400), "{elapsed:?}");
    assert!(elapsed < Duration::from_millis(600), "{elapsed:?}");
}

#[tokio::test]
async fn next_request_null_ends_iteration_only() {
    let server = MockServer::start().await;
    mock_ok(&server).await;
    let dir = setup(&server);
    write(
        dir.path(),
        "a.bru",
        &request(
            "a",
            1,
            "/a",
            &block("script:post-response", "bru.runner.setNextRequest(null);"),
        ),
    );
    write(dir.path(), "b.bru", &request("b", 2, "/b", ""));
    let (report, _) = run_with(dir.path(), Iterations::repeat(2)).await;
    assert_eq!(paths(&server).await, ["/a", "/a"]);
    assert_eq!(report.iterations, 2);
}

#[tokio::test]
async fn stop_execution_stops_run() {
    let server = MockServer::start().await;
    mock_ok(&server).await;
    let dir = setup(&server);
    write(
        dir.path(),
        "a.bru",
        &request(
            "a",
            1,
            "/a",
            &block("script:post-response", "bru.runner.stopExecution();"),
        ),
    );
    write(dir.path(), "b.bru", &request("b", 2, "/b", ""));
    let (report, events) = run_with(dir.path(), Iterations::repeat(3)).await;
    assert_eq!(
        trace(&events),
        ["I0/3", "S0:a", "P0", "K0:stopExecution", "E0", "D"]
    );
    assert_eq!(report.iterations, 1);
}

#[tokio::test]
async fn jump_counter_per_iteration() {
    let server = MockServer::start().await;
    mock_ok(&server).await;
    let dir = setup(&server);
    // `a` jumps to itself 6000 times per iteration, then moves on.
    write(
        dir.path(),
        "a.bru",
        &request(
            "a",
            1,
            "/a",
            &block(
                "script:post-response",
                r#"const n = (bru.getVar("n") || 0) + 1;
bru.setVar("n", n);
if (n % 6000 !== 0) bru.runner.setNextRequest("a");"#,
            ),
        ),
    );
    let c = Collection::load(dir.path()).unwrap();
    let mut failed = 0;
    let opts = RunOptions {
        env: Some("local".into()),
        iterations: Iterations::repeat(2),
        ..RunOptions::default()
    };
    // Only count; keeping 12 000 events is not needed.
    let report = run(&c, dir.path(), &opts, &mut |e| {
        if matches!(e, RunEvent::Failed { .. }) {
            failed += 1;
        }
    })
    .await
    .unwrap();
    assert_eq!(failed, 0);
    assert_eq!(report.passed, 12_000);
}

#[tokio::test]
async fn env_file_untouched() {
    let server = MockServer::start().await;
    mock_ok(&server).await;
    let dir = setup(&server);
    write(dir.path(), "a.bru", &request("a", 1, "/a", ""));
    let env_path = dir.path().join("environments/local.bru");
    let before = fs::read(&env_path).unwrap();
    let rows = vec![row(json!({"host":"http://row.invalid"}))];
    // `host` from the row wins in `{{host}}`, so the request fails to connect...
    let (report, _) = run_with(dir.path(), Iterations::from_rows(rows)).await;
    assert_eq!(report.failed, 1);
    // ...but the environment file is never written.
    assert_eq!(fs::read(&env_path).unwrap(), before);
}
