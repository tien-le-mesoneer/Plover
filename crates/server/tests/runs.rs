mod common;

use std::time::Duration;

use brs_core::executor::ExecOptions;
use brs_server::{RunningServer, ServerConfig, start};
use common::{small_collection, write};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use wiremock::matchers::path_regex;
use wiremock::{Mock, MockServer, ResponseTemplate};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn server() -> RunningServer {
    let (running, fut) = start(
        ServerConfig {
            port: 0,
            dev_origins: Vec::new(),
            exec: ExecOptions::default(),
        },
        std::future::pending(),
    )
    .await
    .unwrap();
    tokio::spawn(fut);
    running
}

async fn connect(running: &RunningServer, protocol: Option<String>) -> Result<Socket, String> {
    let mut req = format!("ws://{}/api/ws", running.addr)
        .into_client_request()
        .unwrap();
    if let Some(p) = protocol {
        req.headers_mut()
            .insert("sec-websocket-protocol", p.parse().unwrap());
    }
    tokio_tungstenite::connect_async(req)
        .await
        .map(|(socket, res)| {
            assert_eq!(
                res.headers()
                    .get("sec-websocket-protocol")
                    .and_then(|v| v.to_str().ok()),
                Some(format!("brs.token.{}", running.token).as_str())
            );
            socket
        })
        .map_err(|e| e.to_string())
}

async fn authed(running: &RunningServer) -> Socket {
    connect(running, Some(format!("brs.token.{}", running.token)))
        .await
        .unwrap()
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
        "type": "start",
        "runId": run_id,
        "collectionId": collection_id,
        "item": item,
        "env": "local",
    });
    socket.send(Message::text(msg.to_string())).await.unwrap();
}

async fn delayed_mock(delay_ms: u64) -> MockServer {
    let mock = MockServer::start().await;
    Mock::given(path_regex("^/[abc]$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("ok")
                .set_delay(Duration::from_millis(delay_ms)),
        )
        .mount(&mock)
        .await;
    mock
}

#[tokio::test]
async fn ws_requires_token_subprotocol() {
    let running = server().await;
    let err = connect(&running, None).await.unwrap_err();
    assert!(err.contains("401"), "{err}");
    let err = connect(&running, Some("brs.token.wrong".into()))
        .await
        .unwrap_err();
    assert!(err.contains("401"), "{err}");
    authed(&running).await;
}

#[tokio::test]
async fn streams_events_in_order() {
    let mock = delayed_mock(0).await;
    let dir = small_collection(&mock.uri());
    let running = server().await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut socket = authed(&running).await;
    start_run(&mut socket, "r1", &id.0, "").await;
    let mut types = Vec::new();
    let mut items = Vec::new();
    loop {
        let event = next_json(&mut socket).await;
        assert_eq!(event["runId"], "r1");
        let kind = event["type"].as_str().unwrap().to_owned();
        if kind == "started" {
            items.push(event["item"].as_str().unwrap().to_owned());
        }
        if kind == "passed" {
            assert_eq!(event["status"], 200);
            assert!(event["elapsedMs"].is_number());
        }
        types.push(kind.clone());
        if kind == "finished" {
            assert_eq!(event["report"]["passed"], 3);
            break;
        }
    }
    assert_eq!(
        types,
        [
            "started", "passed", "started", "passed", "started", "passed", "finished"
        ]
    );
    assert_eq!(items, ["a.bru", "b.bru", "c.bru"]);

    // A run of one folder/request item only runs that item.
    start_run(&mut socket, "r2", &id.0, "b.bru").await;
    assert_eq!(next_json(&mut socket).await["item"], "b.bru");
}

#[tokio::test]
async fn second_start_while_running_is_rejected() {
    let mock = delayed_mock(300).await;
    let dir = small_collection(&mock.uri());
    let running = server().await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut socket = authed(&running).await;
    start_run(&mut socket, "r1", &id.0, "").await;
    start_run(&mut socket, "r2", &id.0, "").await;
    loop {
        let event = next_json(&mut socket).await;
        if event["type"] == "error" {
            assert_eq!(event["runId"], "r2");
            assert_eq!(event["error"], "run_in_progress");
            break;
        }
        assert_eq!(event["runId"], "r1");
    }
}

#[tokio::test]
async fn disconnect_cancels_run() {
    let mock = delayed_mock(300).await;
    let dir = small_collection(&mock.uri());
    let running = server().await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut socket = authed(&running).await;
    start_run(&mut socket, "r1", &id.0, "").await;
    assert_eq!(next_json(&mut socket).await["type"], "started");
    socket.close(None).await.unwrap();
    drop(socket);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let received = mock.received_requests().await.unwrap();
    assert!(
        received.len() <= 1,
        "{} requests after close",
        received.len()
    );
}

#[tokio::test]
async fn cancel_message_stops_run() {
    let mock = delayed_mock(300).await;
    let dir = small_collection(&mock.uri());
    let running = server().await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut socket = authed(&running).await;
    start_run(&mut socket, "r1", &id.0, "").await;
    assert_eq!(next_json(&mut socket).await["type"], "started");
    socket
        .send(Message::text(
            json!({"type": "cancel", "runId": "r1"}).to_string(),
        ))
        .await
        .unwrap();
    let event = next_json(&mut socket).await;
    assert_eq!(event["type"], "cancelled");
    assert_eq!(event["runId"], "r1");
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(mock.received_requests().await.unwrap().len() <= 1);
    // The socket stays usable for a new run.
    start_run(&mut socket, "r2", &id.0, "a.bru").await;
    assert_eq!(next_json(&mut socket).await["runId"], "r2");
}

#[tokio::test]
async fn unknown_environment_is_error_event() {
    let running = server().await;
    let dir = small_collection("http://127.0.0.1:9");
    let id = running.workspace.open(dir.path()).unwrap();
    let mut socket = authed(&running).await;
    let msg =
        json!({"type": "start", "runId": "r1", "collectionId": id.0, "item": "", "env": "nope"});
    socket.send(Message::text(msg.to_string())).await.unwrap();
    let event = next_json(&mut socket).await;
    assert_eq!(event["type"], "error");
    assert_eq!(event["error"], "run_failed");
    let msg =
        json!({"type": "start", "runId": "r2", "collectionId": id.0, "item": "../x", "env": null});
    socket.send(Message::text(msg.to_string())).await.unwrap();
    let event = next_json(&mut socket).await;
    assert_eq!(event["error"], "bad_item");
    socket.send(Message::text("not json")).await.unwrap();
    assert_eq!(next_json(&mut socket).await["error"], "bad_message");
}

#[tokio::test]
async fn run_events_include_tests() {
    let mock = delayed_mock(0).await;
    let dir = small_collection(&mock.uri());
    write(
        dir.path(),
        "a.bru",
        "meta {\n  name: a\n  type: http\n  seq: 1\n}\n\nget {\n  url: {{host}}/a\n}\n\nassert {\n  res.status: eq 201\n}\n\ntests {\n  test('ok', () => expect(res.status).to.equal(200));\n  console.log('ran');\n}\n",
    );
    let running = server().await;
    let id = running.workspace.open(dir.path()).unwrap();
    let mut socket = authed(&running).await;
    start_run(&mut socket, "r1", &id.0, "a.bru").await;
    let mut failed = None;
    loop {
        let event = next_json(&mut socket).await;
        match event["type"].as_str().unwrap() {
            "failed" => failed = Some(event),
            "finished" => {
                let report = &event["report"];
                assert_eq!(
                    (
                        report["testsPassed"].clone(),
                        report["assertsFailed"].clone()
                    ),
                    (1.into(), 1.into())
                );
                break;
            }
            _ => {}
        }
    }
    let failed = failed.expect("a failed event");
    assert_eq!(failed["status"], 200);
    assert_eq!(failed["tests"][0]["name"], "ok");
    assert_eq!(failed["asserts"][0]["passed"], false);
    assert_eq!(failed["asserts"][0]["error"], "expected 200 to equal 201");
    assert_eq!(failed["console"][0]["text"], "ran");
}
