use std::sync::Arc;

use plover_core::executor::ExecOptions;
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
    /// Becomes `true` once the page has mounted (`POST /api/client-ready`).
    pub client_ready: Arc<watch::Sender<bool>>,
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
            client_ready: Arc::new(watch::channel(false).0),
        }
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
