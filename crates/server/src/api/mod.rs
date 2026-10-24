//! REST handlers. Each maps wire DTOs to `plover-core` calls and core errors to `ApiError`s.

mod app_state;
mod collections;
mod curl;
mod data_file;
pub mod dto;
mod environments;
mod extract;
mod history;
mod import;
mod native;
mod requests;
mod session;
pub use requests::MAX_INLINE_BODY;
pub mod runs;

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use plover_core::collection::LoadError;
use plover_core::runner::SendError;
use plover_core::store::StoreError;
use plover_core::workspace::{CollectionId, OpenCollection, WorkspaceError};

use crate::error::ApiError;
use crate::state::AppState;

/// Request bodies carry whole requests (including large JSON bodies).
const MAX_REQUEST_BODY: usize = 64 * 1024 * 1024;

/// Routes that need no session token (Host/Origin checks still apply).
pub fn public_routes() -> Router<AppState> {
    Router::new().route("/session/exchange", post(session::exchange))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/client-ready", post(session::client_ready))
        .route("/events", get(session::events))
        .route(
            "/collections",
            get(collections::list).post(collections::open),
        )
        .route("/collections/{id}", delete(collections::close))
        .route(
            "/recent",
            get(app_state::recent).delete(app_state::remove_recent),
        )
        .route("/ui-state", get(app_state::all_ui_state))
        .route("/layout", get(app_state::layout).put(app_state::put_layout))
        .route(
            "/ui-state/{id}",
            put(app_state::put_ui_state).layer(DefaultBodyLimit::max(app_state::MAX_UI_STATE_BODY)),
        )
        .route("/collections/{id}/tree", get(collections::tree))
        .route("/collections/{id}/reload", post(collections::reload))
        .route(
            "/collections/{id}/requests",
            get(requests::read).put(requests::save),
        )
        .route("/collections/{id}/requests/new", post(requests::create))
        .route("/collections/{id}/send", post(requests::send))
        .route("/curl/parse", post(curl::parse))
        .route("/collections/{id}/curl", post(curl::copy))
        .route("/collections/{id}/resolved-url", post(curl::resolved))
        .route(
            "/collections/{id}/import/har/preview",
            post(import::preview_har),
        )
        .route("/collections/{id}/import/har/plan", post(import::plan_har))
        .route("/collections/{id}/import/har", post(import::import_har))
        .route(
            "/collections/{id}/runs",
            get(history::list).delete(history::clear),
        )
        .route("/collections/{id}/runs/{run_id}", get(history::get))
        .route(
            "/collections/{id}/runs/{run_id}/report",
            get(history::report),
        )
        .route(
            "/collections/{id}/data-file/preview",
            post(data_file::preview),
        )
        .route(
            "/collections/{id}/runs/{run_id}/results/{index}",
            get(history::result),
        )
        .route("/collections/{id}/environments", get(environments::list))
        .route(
            "/collections/{id}/environments/{name}",
            put(environments::save),
        )
        .route(
            "/collections/{id}/environments/{name}/secrets",
            put(environments::set_secrets),
        )
        .route("/capabilities", get(native::capabilities))
        .route("/native/pick-folder", post(native::pick_folder))
        .route("/native/open-browser", post(native::open_browser))
        .route("/native/save-report", post(native::save_report))
        .route("/native/reveal", post(native::reveal))
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
            StoreError::Exists(_) => ApiError::new(StatusCode::CONFLICT, "exists", e.to_string()),
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
