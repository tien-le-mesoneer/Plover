//! Startup and shutdown decisions, kept free of Tauri so they can be unit tested.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long Quit waits for the server to finish before the process exits anyway.
pub const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(3);

/// The native dialog text when the local server cannot start.
pub fn startup_failure_message(err: &std::io::Error) -> String {
    format!("Plover could not start its local server: {err}")
}

/// A collection path passed on the command line (first launch or a second launch
/// forwarded by single-instance): the first argument after the program that isn't a
/// flag, resolved against `cwd`.
pub fn path_from_args(argv: &[String], cwd: &Path) -> Option<PathBuf> {
    argv.iter()
        .skip(1)
        .find(|a| !a.starts_with('-') && !a.is_empty())
        .map(|a| cwd.join(a))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Shutdown {
    /// The server finished (connections closed) before the deadline.
    Graceful,
    /// Something (e.g. an open WebSocket run) kept it busy; the process exits anyway,
    /// which ends in-flight runs and requests.
    TimedOut,
}

/// Waits for the server future (already told to shut down) for at most `deadline`.
pub async fn wait_for_server<F: Future>(server: F, deadline: Duration) -> Shutdown {
    match tokio::time::timeout(deadline, server).await {
        Ok(_) => Shutdown::Graceful,
        Err(_) => Shutdown::TimedOut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_failure_shows_dialog() {
        let err = std::io::Error::new(std::io::ErrorKind::AddrInUse, "address in use");
        assert_eq!(
            startup_failure_message(&err),
            "Plover could not start its local server: address in use"
        );
    }

    #[test]
    fn single_instance_forwards_path() {
        let cwd = Path::new("/home/u");
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
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

    #[tokio::test]
    async fn quit_cancels_runs() {
        // A server kept busy by an open run never finishes on its own: Quit still
        // returns within the deadline, and the process exit then ends the run.
        let started = std::time::Instant::now();
        let outcome =
            wait_for_server(std::future::pending::<()>(), Duration::from_millis(200)).await;
        assert_eq!(outcome, Shutdown::TimedOut);
        assert!(started.elapsed() < Duration::from_millis(1000));
        assert_eq!(
            wait_for_server(async {}, SHUTDOWN_DEADLINE).await,
            Shutdown::Graceful
        );
    }
}
