use std::sync::Arc;

use brs_core::executor::ExecOptions;
use brs_core::workspace::Workspace;

use crate::api::MAX_INLINE_BODY;
use crate::security::AllowList;

#[derive(Clone)]
pub struct AppState {
    pub workspace: Arc<Workspace>,
    pub token: Arc<str>,
    pub allowed: Arc<AllowList>,
}

impl AppState {
    /// The server never reads more of a response body than the UI can show:
    /// without an explicit cap, bodies stop at `MAX_INLINE_BODY`.
    pub fn new(mut exec: ExecOptions, token: &str, allowed: AllowList) -> AppState {
        exec.max_body_bytes.get_or_insert(MAX_INLINE_BODY);
        AppState {
            workspace: Arc::new(Workspace::new(exec)),
            token: Arc::from(token),
            allowed: Arc::new(allowed),
        }
    }
}
