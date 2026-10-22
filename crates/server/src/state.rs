use std::sync::Arc;

use brs_core::workspace::Workspace;

use crate::security::AllowList;

#[derive(Clone)]
pub struct AppState {
    pub workspace: Arc<Workspace>,
    pub token: Arc<str>,
    pub allowed: Arc<AllowList>,
}
