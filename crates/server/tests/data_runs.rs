//! Data-driven runs over the socket, data-file preview, and report export.

mod common;

use std::path::Path;
use std::time::Duration;

use common::write;
use futures_util::{SinkExt, StreamExt};
use plover_core::executor::ExecOptions;
use plover_core::report::{ReportFilter, json};
use plover_core::run_history::RunHistory;
use plover_core::run_record::RunId;
use plover_core::workspace::CollectionId;
use plover_server::{RunningServer, ServerConfig, start};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use wiremock::matchers::path_regex;
use wiremock::{Mock, MockServer, ResponseTemplate};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn server(state_dir: &Path) -> RunningServer {
    let (running, fut) = start(
        ServerConfig {
            exec: ExecOptions::default(),
            state_dir: Some(state_dir.to_path_buf()),
            ..ServerConfig::default()
        },
        std::future::pending(),
    )
    .await
    .unwrap();
    tokio::spawn(fut);
    running
}

async fn socket(running: &RunningServer) -> Socket {
    let mut req = format!("ws://{}/api/ws", running.addr)
        .into_client_request()
        .unwrap();
    req.headers_mut().insert(
        "sec-websocket-protocol",
        format!("plover.token.{}", running.token).parse().unwrap(),
    );
    tokio_tungstenite::connect_async(req).await.unwrap().0
}

async fn next_json(socket: &mut Socket) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), socket.next())
            .await
            .expect("timed out waiting for an event")
            .expect("socket closed")
            .unwrap();
        if let Message::Text(text) = msg {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

async fn send(socket: &mut Socket, msg: Value) {
    socket.send(Message::text(msg.to_string())).await.unwrap();
}

async fn until_end(socket: &mut Socket) -> Vec<Value> {
    let mut events = Vec::new();
    loop {
        let e = next_json(socket).await;
        let end = ["finished", "cancelled", "error"].contains(&e["type"].as_str().unwrap());
        events.push(e);
        if end {
            return events;
        }
    }
}

async fn api(
    running: &RunningServer,
    method: &str,
    rel: &str,
    body: Option<Value>,
) -> reqwest::Response {
    let client = reqwest::Client::new();
    let url = format!("http://{}/api{rel}", running.addr);
    let req = match method {
        "POST" => client
            .post(url)
            .header("content-type", "application/json")
            .body(body.unwrap_or(Value::Null).to_string()),
        _ => client.get(url),
    };
    req.bearer_auth(&running.token).send().await.unwrap()
}

/// `a` GET `/a/{{user}}`, `b` GET `/b`; the password goes in a header of `b`.
fn collection(host: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"Data API","type":"collection"}"#,
    );
    write(
        root,
        "environments/local.bru",
        &format!("vars {{\n  host: {host}\n}}\n"),
    );
    write(
        root,
        "a.bru",
        "meta {\n  name: a\n  type: http\n  seq: 1\n}\n\nget {\n  url: {{host}}/a/{{user}}\n}\n",
    );
    write(
        root,
        "b.bru",
        "meta {\n  name: b\n  type: http\n  seq: 2\n}\n\nget {\n  url: {{host}}/b\n}\n\nheaders {\n  x-pw: {{password}}\n}\n",
    );
    dir
}

async fn mock(delay_ms: u64) -> MockServer {
    let mock = MockServer::start().await;
    Mock::given(path_regex(".*"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("{}")
                .set_delay(Duration::from_millis(delay_ms)),
        )
        .mount(&mock)
        .await;
    mock
}

const CSV: &str = "user,password\nalice,pw-SECRET-1\nbob,pw-SECRET-2\n";

fn start_msg(collection: &str, extra: Value) -> Value {
    let mut msg = json!({"type": "start", "runId": "r1", "collectionId": collection, "item": "", "env": "local"});
    msg.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    msg
}

#[tokio::test]
async fn start_with_csv_runs_rows() {
    let m = mock(0).await;
    let dir = collection(&m.uri());
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut s = socket(&running).await;
    send(
        &mut s,
        start_msg(&id.0, json!({"data": {"format": "csv", "text": CSV}})),
    )
    .await;
    let events = until_end(&mut s).await;
    let kinds: Vec<String> = events
        .iter()
        .map(|e| match e["type"].as_str().unwrap() {
            "iterationStarted" => format!("I{}/{}", e["index"], e["total"]),
            "started" | "passed" => {
                format!("{}{}", &e["type"].as_str().unwrap()[..1], e["iteration"])
            }
            other => other.to_owned(),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "accepted", "I0/2", "s0", "p0", "s0", "p0", "I1/2", "s1", "p1", "s1", "p1", "finished"
        ]
    );
    assert_eq!(events.last().unwrap()["report"]["iterations"], 2);
    let paths: Vec<String> = m
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_owned())
        .collect();
    assert_eq!(paths, ["/a/alice", "/b", "/a/bob", "/b"]);
}

#[tokio::test]
async fn iteration_started_data_masked() {
    let m = mock(0).await;
    let dir = collection(&m.uri());
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut s = socket(&running).await;
    send(
        &mut s,
        start_msg(&id.0, json!({"data": {"format": "csv", "text": CSV}})),
    )
    .await;
    let events = until_end(&mut s).await;
    let first = events
        .iter()
        .find(|e| e["type"] == "iterationStarted")
        .unwrap();
    assert_eq!(first["data"], json!({"user": "alice", "password": "***"}));
    assert!(
        !serde_json::to_string(&events)
            .unwrap()
            .contains("pw-SECRET")
    );
}

#[tokio::test]
async fn bad_data_is_an_error_event() {
    let m = mock(0).await;
    let dir = collection(&m.uri());
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut s = socket(&running).await;
    send(
        &mut s,
        start_msg(&id.0, json!({"data": {"format": "json", "text": "{}"}})),
    )
    .await;
    let e = next_json(&mut s).await;
    assert_eq!(e["type"], "error");
    assert_eq!(e["error"], "bad_data");
    assert!(e["message"].as_str().unwrap().contains("array"), "{e}");
    let big = "a".repeat((16 << 20) + 1);
    send(
        &mut s,
        start_msg(&id.0, json!({"data": {"format": "csv", "text": big}})),
    )
    .await;
    assert_eq!(next_json(&mut s).await["error"], "bad_data");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(m.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn iteration_count_runs_n_times() {
    let m = mock(0).await;
    let dir = collection(&m.uri());
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut s = socket(&running).await;
    send(&mut s, start_msg(&id.0, json!({"iterationCount": 3}))).await;
    let events = until_end(&mut s).await;
    assert_eq!(events.last().unwrap()["report"]["iterations"], 3);
    assert_eq!(m.received_requests().await.unwrap().len(), 6);
}

#[tokio::test]
async fn preview_returns_counts_not_values() {
    let m = mock(0).await;
    let dir = collection(&m.uri());
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let res = api(
        &running,
        "POST",
        &format!("/collections/{}/data-file/preview", id.0),
        Some(json!({"format": "csv", "text": CSV})),
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
    let text = res.text().await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        json!({"rows": 2, "columns": ["user", "password"]})
    );
    assert!(!text.contains("alice") && !text.contains("pw-SECRET"));
    let res = api(
        &running,
        "POST",
        &format!("/collections/{}/data-file/preview", id.0),
        Some(json!({"format": "csv", "text": "a,b\n1\n"})),
    )
    .await;
    assert_eq!(res.status().as_u16(), 400);
    let body: Value = serde_json::from_str(&res.text().await.unwrap()).unwrap();
    assert_eq!(body["error"], "bad_data");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains(":2: expected 2 fields"),
        "{body}"
    );
}

#[tokio::test]
async fn cancel_mid_iteration_saves_partial() {
    let m = mock(150).await;
    let dir = collection(&m.uri());
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut s = socket(&running).await;
    let csv = "user\nu1\nu2\nu3\nu4\nu5\n";
    send(
        &mut s,
        start_msg(&id.0, json!({"data": {"format": "csv", "text": csv}})),
    )
    .await;
    let accepted = next_json(&mut s).await;
    let record_id = accepted["recordId"].as_str().unwrap().to_owned();
    loop {
        let e = next_json(&mut s).await;
        if e["type"] == "iterationStarted" && e["index"] == 2 {
            break;
        }
    }
    send(&mut s, json!({"type": "cancel", "runId": "r1"})).await;
    loop {
        if next_json(&mut s).await["type"] == "cancelled" {
            break;
        }
    }
    let record = RunHistory::at(state.path())
        .load(&id, &RunId::parse(&record_id).unwrap())
        .unwrap();
    assert_eq!(record.outcome.as_str(), "cancelled");
    assert_eq!(record.iterations.len(), 3);
    let res = api(
        &running,
        "GET",
        &format!("/collections/{}/runs/{record_id}/report?format=json", id.0),
        None,
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
    let report: Value = serde_json::from_str(&res.text().await.unwrap()).unwrap();
    assert_eq!(report.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn export_matches_plv_show() {
    let m = mock(0).await;
    let dir = collection(&m.uri());
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut s = socket(&running).await;
    send(
        &mut s,
        start_msg(&id.0, json!({"data": {"format": "csv", "text": CSV}})),
    )
    .await;
    let events = until_end(&mut s).await;
    let record_id = events[0]["recordId"].as_str().unwrap().to_owned();
    let record = RunHistory::at(state.path())
        .load(
            &CollectionId(id.0.clone()),
            &RunId::parse(&record_id).unwrap(),
        )
        .unwrap();
    let res = api(
        &running,
        "GET",
        &format!("/collections/{}/runs/{record_id}/report?format=json", id.0),
        None,
    )
    .await;
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(
        res.headers()["content-disposition"],
        format!("attachment; filename=\"Data-API-{record_id}.json\"").as_str()
    );
    let body = res.text().await.unwrap();
    // `plv runs show --reporter-json` writes exactly this.
    assert_eq!(
        body,
        serde_json::to_string_pretty(&json::render(&record, &ReportFilter::default())).unwrap()
    );
    assert!(!body.contains("pw-SECRET"));
    let res = api(
        &running,
        "GET",
        &format!("/collections/{}/runs/{record_id}/report?format=html", id.0),
        None,
    )
    .await;
    assert!(
        res.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .ends_with(".html\"")
    );
    assert!(res.text().await.unwrap().contains("Iteration 2 of 2"));
    let res = api(
        &running,
        "GET",
        &format!("/collections/{}/runs/{record_id}/report?format=pdf", id.0),
        None,
    )
    .await;
    assert_eq!(res.status().as_u16(), 400);
}

#[tokio::test]
async fn export_while_running_is_409() {
    let m = mock(400).await;
    let dir = collection(&m.uri());
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut s = socket(&running).await;
    send(&mut s, start_msg(&id.0, json!({}))).await;
    let accepted = next_json(&mut s).await;
    let record_id = accepted["recordId"].as_str().unwrap();
    let res = api(
        &running,
        "GET",
        &format!("/collections/{}/runs/{record_id}/report?format=json", id.0),
        None,
    )
    .await;
    assert_eq!(res.status().as_u16(), 409);
    let body: Value = serde_json::from_str(&res.text().await.unwrap()).unwrap();
    assert_eq!(body["error"], "run_in_progress");
}
