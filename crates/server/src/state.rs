use std::sync::Arc;

use plover_core::executor::ExecOptions;
use plover_core::secrets::SecretVault;
use plover_core::workspace::Workspace;

use crate::api::MAX_INLINE_BODY;
use crate::native::NativeHost;
use crate::security::AllowList;

#[derive(Clone)]
pub struct AppState {
    pub workspace: Arc<Workspace>,
    pub token: Arc<str>,
    pub allowed: Arc<AllowList>,
    /// The desktop shell's native features; `None` in web mode.
    pub native: Option<Arc<dyn NativeHost>>,
    /// `http://127.0.0.1:<port>/#token=<token>`, for "Open in browser".
    pub session_url: Arc<str>,
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
            session_url: Arc::from(""),
        }
    }

    pub fn with_native(mut self, host: Arc<dyn NativeHost>) -> AppState {
        self.native = Some(host);
        self
    }

    pub fn with_session_url(mut self, url: &str) -> AppState {
        self.session_url = Arc::from(url);
        self
    }
}
