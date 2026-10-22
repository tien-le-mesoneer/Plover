//! REST handlers. Each maps wire DTOs to `brs-core` calls and core errors to `ApiError`s.

mod collections;
pub mod dto;
mod environments;
mod extract;
mod requests;
pub use requests::MAX_INLINE_BODY;
pub mod runs;

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use brs_core::collection::LoadError;
use brs_core::runner::SendError;
use brs_core::store::StoreError;
use brs_core::workspace::{CollectionId, OpenCollection, WorkspaceError};

use crate::error::ApiError;
use crate::state::AppState;

/// Request bodies carry whole requests (including large JSON bodies).
const MAX_REQUEST_BODY: usize = 64 * 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/collections",
            get(collections::list).post(collections::open),
        )
        .route("/collections/{id}/tree", get(collections::tree))
        .route("/collections/{id}/reload", post(collections::reload))
        .route(
            "/collections/{id}/requests",
            get(requests::read).put(requests::save),
        )
        .route("/collections/{id}/send", post(requests::send))
        .route("/collections/{id}/environments", get(environments::list))
        .route(
            "/collections/{id}/environments/{name}",
            put(environments::save),
        )
        .route(
            "/collections/{id}/environments/{name}/secrets",
            put(environments::set_secrets),
        )
        .route("/ws", get(runs::upgrade))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY))
}

fn open_collection(state: &AppState, id: &str) -> Result<Arc<OpenCollection>, ApiError> {
    state
        .workspace
        .get(&CollectionId(id.to_owned()))
        .ok_or_else(|| ApiError::not_found(format!("collection `{id}` is not open")))
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::OutsideRoot(_) => {
                ApiError::new(StatusCode::BAD_REQUEST, "bad_item", e.to_string())
            }
            StoreError::NotFound(_) => ApiError::not_found(e.to_string()),
            StoreError::Parse(_) => ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "parse_error",
                e.to_string(),
            ),
            StoreError::Conflict { current } => ApiError {
                status: StatusCode::CONFLICT,
                code: "conflict",
                message: "the file changed on disk since it was read".to_owned(),
                extra: Some(serde_json::json!({
                    "current": dto::ConflictDto { text: current.value, hash: current.hash.0 }
                })),
            },
            StoreError::Io(_) => {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "io_error", e.to_string())
            }
        }
    }
}

impl From<LoadError> for ApiError {
    fn from(e: LoadError) -> Self {
        match e {
            LoadError::NotACollection(_) => ApiError::not_found(e.to_string()),
            LoadError::Config { .. } => ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "parse_error",
                e.to_string(),
            ),
            LoadError::Io { .. } => {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "io_error", e.to_string())
            }
        }
    }
}

impl From<WorkspaceError> for ApiError {
    fn from(e: WorkspaceError) -> Self {
        match e {
            WorkspaceError::Load(e) => e.into(),
            WorkspaceError::Session(e) => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session_error",
                e.to_string(),
            ),
        }
    }
}

impl From<SendError> for ApiError {
    fn from(e: SendError) -> Self {
        match e {
            SendError::Store(e) => e.into(),
            SendError::UnknownEnvironment(_) | SendError::NotFound(_) => {
                ApiError::not_found(e.to_string())
            }
            SendError::Unsupported(_) | SendError::Resolve(_) => ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "resolve_error",
                e.to_string(),
            ),
            SendError::Exec(_) => {
                ApiError::new(StatusCode::BAD_GATEWAY, "send_failed", e.to_string())
            }
        }
    }
}
