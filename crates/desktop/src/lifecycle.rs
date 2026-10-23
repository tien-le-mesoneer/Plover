//! Starting and stopping the local server, kept free of Tauri so it can be tested.
//! The server runs on its own Tokio runtime: Quit shuts that runtime down, which
//! cancels every connection and in-flight run.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use plover_server::{RunningServer, ServerConfig};

/// How long Quit waits for the server to finish before cancelling what is left.
pub const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(3);

/// The native dialog text when the local server cannot start.
pub fn startup_failure_message(err: &std::io::Error) -> String {
    format!("Plover could not start its local server: {err}")
}

/// A collection path passed on the command line (first launch or a second launch
/// forwarded by single-instance): the first argument after the program that isn't a
/// flag, resolved against `cwd`. Works with non-UTF-8 arguments.
pub fn path_from_args(argv: &[OsString], cwd: &Path) -> Option<PathBuf> {
    argv.iter()
        .skip(1)
        .find(|a| !a.is_empty() && !a.as_encoded_bytes().starts_with(b"-"))
        .map(|a| cwd.join(a))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Shutdown {
    /// The server future finished before the deadline. Upgraded WebSocket connections
    /// (runs, events) are not tracked by it; the runtime shutdown cancels them anyway.
    Graceful,
    /// It was still busy at the deadline; its tasks were cancelled.
    Cancelled,
}

/// The server and the runtime it runs on.
pub struct ServerRuntime {
    runtime: tokio::runtime::Runtime,
    shutdown: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
}

/// Starts the server on a new runtime. The error is the dialog text.
pub fn launch(cfg: ServerConfig) -> Result<(ServerRuntime, RunningServer), String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("plover-server")
        .build()
        .map_err(|e| startup_failure_message(&e))?;
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let (running, server) = runtime
        .block_on(plover_server::start(cfg, async {
            let _ = rx.await;
        }))
        .map_err(|e| startup_failure_message(&e))?;
    let server = runtime.spawn(server);
    Ok((
        ServerRuntime {
            runtime,
            shutdown: tx,
            server,
        },
        running,
    ))
}

impl ServerRuntime {
    /// Asks the server to stop, waits at most `deadline`, then shuts the runtime down,
    /// cancelling any connection or run still in flight.
    pub fn stop(self, deadline: Duration) -> Shutdown {
        let _ = self.shutdown.send(());
        let finished = self
            .runtime
            .block_on(async { tokio::time::timeout(deadline, self.server).await.is_ok() });
        self.runtime.shutdown_timeout(Duration::from_millis(200));
        if finished {
            Shutdown::Graceful
        } else {
            Shutdown::Cancelled
        }
    }

    /// Runs `future` on the server's runtime (e.g. the smoke test's checks).
    pub fn spawn<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.runtime.spawn(future);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn start_failure_shows_dialog() {
        // The real startup path, with the port already taken.
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = taken.local_addr().unwrap().port();
        let err = launch(ServerConfig {
            port,
            ..ServerConfig::default()
        })
        .err()
        .expect("binding a taken port fails");
        assert!(
            err.starts_with("Plover could not start its local server: "),
            "{err}"
        );
    }

    #[test]
    fn single_instance_forwards_path() {
        let cwd = Path::new("/home/u");
        let args = |a: &[&str]| a.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(path_from_args(&args(&["plover"]), cwd), None);
        assert_eq!(
            path_from_args(&args(&["plover", "--flag", "coll"]), cwd),
            Some(PathBuf::from("/home/u/coll"))
        );
        assert_eq!(
            path_from_args(&args(&["plover", "/abs/coll"]), cwd),
            Some(PathBuf::from("/abs/coll"))
        );
        // macOS adds -psn_… when launched from Finder.
        assert_eq!(path_from_args(&args(&["plover", "-psn_0_1234"]), cwd), None);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_arguments_do_not_abort() {
        use std::os::unix::ffi::OsStringExt;
        let odd = OsString::from_vec(b"coll-\xff".to_vec());
        let got = path_from_args(&[OsString::from("plover"), odd.clone()], Path::new("/u"));
        assert_eq!(got, Some(Path::new("/u").join(odd)));
    }

    #[test]
    fn quit_cancels_runs() {
        let mock_rt = tokio::runtime::Runtime::new().unwrap();
        let mock = mock_rt.block_on(async {
            let mock = MockServer::start().await;
            Mock::given(path("/slow"))
                .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(700)))
                .mount(&mock)
                .await;
            mock
        });
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("bruno.json"),
            r#"{"version":"1","name":"q","type":"collection"}"#,
        )
        .unwrap();
        for i in 1..=6 {
            std::fs::write(
                dir.path().join(format!("r{i}.bru")),
                format!(
                    "meta {{\n  name: r{i}\n  type: http\n  seq: {i}\n}}\n\nget {{\n  url: {}/slow\n  body: none\n  auth: none\n}}\n",
                    mock.uri()
                ),
            )
            .unwrap();
        }
        let (server, running) = launch(ServerConfig::default()).unwrap();
        let id = running.workspace.open(dir.path()).unwrap();

        // Start a run over the WebSocket, as the UI does, and keep the socket open.
        let addr = running.addr;
        let token = running.token.clone();
        let _socket = mock_rt.block_on(async move {
            let mut req = format!("ws://{addr}/api/ws").into_client_request().unwrap();
            req.headers_mut().insert(
                "sec-websocket-protocol",
                format!("plover.token.{token}").parse().unwrap(),
            );
            let (mut socket, _) = tokio_tungstenite::connect_async(req).await.unwrap();
            let start = serde_json::json!({
                "type": "start", "runId": "r", "collectionId": id.0, "item": "", "env": null
            });
            socket
                .send(Message::Text(start.to_string().into()))
                .await
                .unwrap();
            socket
        });
        let received = || {
            mock_rt
                .block_on(mock.received_requests())
                .unwrap_or_default()
                .len()
        };
        let started = std::time::Instant::now();
        while received() == 0 {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "run never started"
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        let quit = std::time::Instant::now();
        server.stop(Duration::from_millis(300));
        assert!(quit.elapsed() < Duration::from_secs(2));
        let at_quit = received();
        std::thread::sleep(Duration::from_millis(1600));
        assert_eq!(received(), at_quit, "no request may start after Quit");
        assert!(at_quit < 6, "the run was cut short ({at_quit} of 6 sent)");
    }
}
