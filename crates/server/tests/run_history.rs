//! App runs are recorded, masked and saved, and served by the run history endpoints.

mod common;

use std::path::Path;
use std::time::Duration;

use common::{small_collection, write};
use futures_util::{SinkExt, StreamExt};
use plover_core::executor::ExecOptions;
use plover_server::{RunningServer, ServerConfig, start};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use wiremock::matchers::{path, path_regex};
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

async fn start_run(socket: &mut Socket, run_id: &str, collection_id: &str, item: &str) {
    let msg = json!({
        "type": "start", "runId": run_id, "collectionId": collection_id, "item": item, "env": "local",
    });
    socket.send(Message::text(msg.to_string())).await.unwrap();
}

/// Events until (and including) `finished` or `cancelled`.
async fn until_end(socket: &mut Socket) -> Vec<Value> {
    let mut events = Vec::new();
    loop {
        let e = next_json(socket).await;
        let end = e["type"] == "finished" || e["type"] == "cancelled";
        events.push(e);
        if end {
            return events;
        }
    }
}

async fn api(running: &RunningServer, method: &str, rel: &str) -> (u16, Value) {
    let client = reqwest::Client::new();
    let url = format!("http://{}/api{rel}", running.addr);
    let req = match method {
        "DELETE" => client.delete(url),
        _ => client.get(url),
    };
    let res = req.bearer_auth(&running.token).send().await.unwrap();
    let status = res.status().as_u16();
    let bytes = res.bytes().await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

async fn ok_mock() -> MockServer {
    let mock = MockServer::start().await;
    Mock::given(path_regex("^/[abc]$"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&mock)
        .await;
    mock
}

fn accepted_id(events: &[Value]) -> String {
    assert_eq!(events[0]["type"], "accepted", "{events:?}");
    events[0]["recordId"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn accepted_then_indexed_events() {
    let mock = ok_mock().await;
    let dir = small_collection(&mock.uri());
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut s = socket(&running).await;
    start_run(&mut s, "r1", &id.0, "").await;
    let events = until_end(&mut s).await;
    let record_id = accepted_id(&events);
    assert!(plover_core::run_record::RunId::parse(&record_id).is_some());
    let indices: Vec<u64> = events
        .iter()
        .filter(|e| e["type"] == "passed")
        .map(|e| e["index"].as_u64().unwrap())
        .collect();
    assert_eq!(indices, [0, 1, 2]);
}

#[tokio::test]
async fn detail_while_running_and_after() {
    let mock = MockServer::start().await;
    Mock::given(path("/a"))
        .respond_with(ResponseTemplate::new(200).set_body_string("first"))
        .mount(&mock)
        .await;
    Mock::given(path_regex("^/[bc]$"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(600)))
        .mount(&mock)
        .await;
    let dir = small_collection(&mock.uri());
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut s = socket(&running).await;
    start_run(&mut s, "r1", &id.0, "").await;
    let record_id = accepted_id(&[next_json(&mut s).await]);
    loop {
        if next_json(&mut s).await["type"] == "passed" {
            break;
        }
    }
    // Result 1 is in flight: result 0 comes from memory.
    let uri = format!("/collections/{}/runs/{record_id}/results/0", id.0);
    let (status, live) = api(&running, "GET", &uri).await;
    assert_eq!(status, 200, "{live}");
    assert_eq!(live["result"]["response"]["bodyText"], "first");
    assert_eq!(live["request"]["method"], "GET");
    until_end(&mut s).await;
    let file = state
        .path()
        .join("runs")
        .join(&id.0)
        .join(format!("{record_id}.json"));
    assert!(file.is_file(), "{}", file.display());
    let (status, saved) = api(&running, "GET", &uri).await;
    assert_eq!(status, 200);
    assert_eq!(saved, live);
}

#[tokio::test]
async fn finished_is_sent_after_masking() {
    let mock = MockServer::start().await;
    Mock::given(path("/a"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"token": "tok-AAAABBBBCCCC"})),
        )
        .mount(&mock)
        .await;
    Mock::given(path_regex("^/[bc]$"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock)
        .await;
    let dir = small_collection(&mock.uri());
    write(
        dir.path(),
        "a.bru",
        "meta {\n  name: a\n  type: http\n  seq: 1\n}\n\nget {\n  url: {{host}}/a\n}\n\nscript:post-response {\n  bru.setVar('tok', res.body.token);\n}\n",
    );
    write(
        dir.path(),
        "b.bru",
        "meta {\n  name: b\n  type: http\n  seq: 2\n}\n\nget {\n  url: {{host}}/b\n}\n\nheaders {\n  Authorization: Bearer {{tok}}\n}\n",
    );
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut s = socket(&running).await;
    start_run(&mut s, "r1", &id.0, "").await;
    let events = until_end(&mut s).await;
    let record_id = accepted_id(&events);
    let (_, detail) = api(
        &running,
        "GET",
        &format!("/collections/{}/runs/{record_id}/results/0", id.0),
    )
    .await;
    let body = detail["result"]["response"]["bodyText"].as_str().unwrap();
    assert!(body.contains("***"), "{body}");
    assert!(!body.contains("tok-AAAABBBBCCCC"));
    let saved = std::fs::read_to_string(
        state
            .path()
            .join("runs")
            .join(&id.0)
            .join(format!("{record_id}.json")),
    )
    .unwrap();
    assert!(!saved.contains("tok-AAAABBBBCCCC"));
}

async fn wait_for_runs(running: &RunningServer, collection: &str, n: usize) -> Vec<Value> {
    for _ in 0..50 {
        let (_, list) = api(running, "GET", &format!("/collections/{collection}/runs")).await;
        let list = list.as_array().unwrap().clone();
        if list.len() >= n {
            return list;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("fewer than {n} runs saved");
}

#[tokio::test]
async fn cancelled_run_is_saved() {
    let mock = MockServer::start().await;
    Mock::given(path("/a"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock)
        .await;
    Mock::given(path_regex("^/[bc]$"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&mock)
        .await;
    let dir = small_collection(&mock.uri());
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    // Cancel message.
    let mut s = socket(&running).await;
    start_run(&mut s, "r1", &id.0, "").await;
    loop {
        if next_json(&mut s).await["type"] == "passed" {
            break;
        }
    }
    s.send(Message::text(
        json!({"type": "cancel", "runId": "r1"}).to_string(),
    ))
    .await
    .unwrap();
    assert_eq!(until_end(&mut s).await.last().unwrap()["type"], "cancelled");
    let list = wait_for_runs(&running, &id.0, 1).await;
    assert_eq!(list[0]["outcome"], "cancelled");
    assert_eq!(list[0]["report"]["passed"], 1);
    // Socket disconnect.
    let mut s = socket(&running).await;
    start_run(&mut s, "r2", &id.0, "").await;
    loop {
        if next_json(&mut s).await["type"] == "passed" {
            break;
        }
    }
    s.close(None).await.unwrap();
    drop(s);
    let list = wait_for_runs(&running, &id.0, 2).await;
    assert!(list.iter().all(|r| r["outcome"] == "cancelled"));
}

#[tokio::test]
async fn list_get_clear() {
    let mock = ok_mock().await;
    let dir = small_collection(&mock.uri());
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut s = socket(&running).await;
    start_run(&mut s, "r1", &id.0, "").await;
    let first = accepted_id(&until_end(&mut s).await);
    start_run(&mut s, "r2", &id.0, "b.bru").await;
    let second = accepted_id(&until_end(&mut s).await);
    let (status, list) = api(&running, "GET", &format!("/collections/{}/runs", id.0)).await;
    assert_eq!(status, 200);
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["recordId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [second.as_str(), first.as_str()]);
    assert_eq!(list[0]["source"], "app");
    assert_eq!(list[0]["target"], "b.bru");
    assert_eq!(list[0]["env"], "local");
    let (status, run) = api(
        &running,
        "GET",
        &format!("/collections/{}/runs/{first}", id.0),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(run["label"], "small");
    let types: Vec<&str> = run["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        types,
        [
            "started", "passed", "started", "passed", "started", "passed", "finished"
        ]
    );
    assert_eq!(run["events"][3]["index"], 1);
    assert_eq!(run["events"][1]["runId"], first.as_str());
    let (status, _) = api(&running, "DELETE", &format!("/collections/{}/runs", id.0)).await;
    assert_eq!(status, 204);
    let (_, list) = api(&running, "GET", &format!("/collections/{}/runs", id.0)).await;
    assert_eq!(list, json!([]));
}

#[tokio::test]
async fn bad_ids_rejected() {
    let mock = ok_mock().await;
    let dir = small_collection(&mock.uri());
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let (status, body) = api(
        &running,
        "GET",
        &format!("/collections/{}/runs/..%2Fx", id.0),
    )
    .await;
    assert_eq!((status, body["error"].clone()), (400, json!("bad_run_id")));
    let mut s = socket(&running).await;
    start_run(&mut s, "r1", &id.0, "a.bru").await;
    let record_id = accepted_id(&until_end(&mut s).await);
    let (status, _) = api(
        &running,
        "GET",
        &format!("/collections/{}/runs/{record_id}/results/7", id.0),
    )
    .await;
    assert_eq!(status, 404);
    let (status, _) = api(&running, "GET", "/collections/nope/runs").await;
    assert_eq!(status, 404);
    let (status, _) = api(
        &running,
        "GET",
        &format!("/collections/{}/runs/19700102T000000Z-abcdef", id.0),
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn repeated_item_has_distinct_details() {
    let mock = MockServer::start().await;
    Mock::given(path("/a"))
        .respond_with(ResponseTemplate::new(200).set_body_string("one"))
        .up_to_n_times(1)
        .mount(&mock)
        .await;
    Mock::given(path("/a"))
        .respond_with(ResponseTemplate::new(200).set_body_string("two"))
        .mount(&mock)
        .await;
    Mock::given(path_regex("^/[bc]$"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock)
        .await;
    let dir = small_collection(&mock.uri());
    write(
        dir.path(),
        "a.bru",
        "meta {\n  name: a\n  type: http\n  seq: 1\n}\n\nget {\n  url: {{host}}/a\n}\n\nscript:post-response {\n  if (!bru.getVar('again')) {\n    bru.setVar('again', 1);\n    bru.runner.setNextRequest('a');\n  }\n}\n",
    );
    let state = tempfile::tempdir().unwrap();
    let running = server(state.path()).await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut s = socket(&running).await;
    start_run(&mut s, "r1", &id.0, "").await;
    let events = until_end(&mut s).await;
    let record_id = accepted_id(&events);
    let detail = |i: usize| {
        let running = &running;
        let uri = format!("/collections/{}/runs/{record_id}/results/{i}", id.0);
        async move { api(running, "GET", &uri).await.1 }
    };
    let (zero, one) = (detail(0).await, detail(1).await);
    assert_eq!(
        (zero["item"].clone(), one["item"].clone()),
        (json!("a.bru"), json!("a.bru"))
    );
    assert_eq!(zero["result"]["response"]["bodyText"], "one");
    assert_eq!(one["result"]["response"]["bodyText"], "two");
}
