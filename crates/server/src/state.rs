use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use plover_core::executor::ExecOptions;
use plover_core::run_history::RunHistory;
use plover_core::run_record::{RunRecord, RunRecorder};
use plover_core::secrets::SecretVault;
use plover_core::workspace::Workspace;
use tokio::sync::watch;

use crate::api::MAX_INLINE_BODY;
use crate::events::Events;
use crate::native::NativeHost;
use crate::security::AllowList;
use crate::session::SessionCodes;

#[derive(Clone)]
pub struct AppState {
    pub workspace: Arc<Workspace>,
    pub token: Arc<str>,
    pub allowed: Arc<AllowList>,
    /// The desktop shell's native features; `None` in web mode.
    pub native: Option<Arc<dyn NativeHost>>,
    /// `http://127.0.0.1:<port>`, for launch URLs.
    pub base_url: Arc<str>,
    pub codes: Arc<SessionCodes>,
    pub events: Events,
    /// How far the page got: loaded, launch code traded, ready report (the desktop smoke
    /// test reads it to say where startup stopped).
    pub client: Arc<watch::Sender<ClientProgress>>,
    /// Saved runs; in memory when the server has no state directory.
    pub history: Arc<RunHistory>,
    /// Runs in progress or being saved, by record ID, so their results can be shown.
    pub live: Arc<LiveRuns>,
}

/// A run the server holds in memory: still recording, or finished and being saved.
pub enum LiveRun {
    Running(Box<RunRecorder>),
    Done(Arc<RunRecord>),
}

pub type LiveRuns = Mutex<HashMap<String, LiveRun>>;

/// Startup milestones of the UI page. Counts, so a page that loads twice (and then holds
/// an already-used launch code) is visible.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientProgress {
    pub page_loads: u32,
    pub codes_exchanged: u32,
    pub codes_rejected: u32,
    /// `POST /api/client-ready`: `Ok` or the error the page reported.
    pub ready: Option<Result<(), String>>,
}

impl ClientProgress {
    /// What the page got through, for a failure message.
    pub fn summary(&self) -> String {
        format!(
            "page loads: {}, launch codes exchanged: {}, rejected: {}",
            self.page_loads, self.codes_exchanged, self.codes_rejected
        )
    }
}

impl AppState {
    /// Secrets are kept in memory; see [`AppState::with_secrets`].
    pub fn new(exec: ExecOptions, token: &str, allowed: AllowList) -> AppState {
        AppState::with_secrets(exec, token, allowed, Arc::new(SecretVault::memory()))
    }

    /// The server never reads more of a response body than the UI can show:
    /// without an explicit cap, bodies stop at `MAX_INLINE_BODY`.
    pub fn with_secrets(
        mut exec: ExecOptions,
        token: &str,
        allowed: AllowList,
        secrets: Arc<SecretVault>,
    ) -> AppState {
        exec.max_body_bytes.get_or_insert(MAX_INLINE_BODY);
        AppState {
            workspace: Arc::new(Workspace::with_secrets(exec, secrets)),
            token: Arc::from(token),
            allowed: Arc::new(allowed),
            native: None,
            base_url: Arc::from(""),
            codes: Arc::new(SessionCodes::default()),
            events: Events::default(),
            client: Arc::new(watch::channel(ClientProgress::default()).0),
            history: Arc::new(RunHistory::in_memory()),
            live: Arc::default(),
        }
    }

    pub fn with_history(mut self, history: RunHistory) -> AppState {
        self.history = Arc::new(history);
        self
    }

    pub fn with_native(mut self, host: Arc<dyn NativeHost>) -> AppState {
        self.native = Some(host);
        self
    }

    pub fn with_base_url(mut self, url: &str) -> AppState {
        self.base_url = Arc::from(url);
        self
    }

    pub fn with_codes(mut self, codes: SessionCodes) -> AppState {
        self.codes = Arc::new(codes);
        self
    }

    /// `<base>/#code=<fresh single-use code>`.
    pub fn launch_url(&self) -> String {
        format!("{}/#code={}", self.base_url, self.codes.mint())
    }
}
