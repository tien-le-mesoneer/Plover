//! What the server remembers between starts: recently opened collections and each
//! collection's UI layout. Without a state store (tests, embedders) nothing is kept.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use plover_core::workspace::CollectionId;
use serde::Deserialize;

use super::collections::display_path;
use super::dto::{RecentDto, UiStateDto};
use super::extract::{ApiJson, ApiPath, ApiQuery};
use crate::error::ApiError;
use crate::state::AppState;

/// The largest UI-state body accepted (`PUT /api/ui-state/{id}`).
pub const MAX_UI_STATE_BODY: usize = 64 * 1024;

pub async fn recent(State(state): State<AppState>) -> Json<Vec<RecentDto>> {
    let Some(store) = state.workspace.app_state() else {
        return Json(Vec::new());
    };
    let open: Vec<String> = state
        .workspace
        .list()
        .iter()
        .map(|o| o.canonical_root.clone())
        .collect();
    Json(
        store
            .snapshot()
            .recent
            .into_iter()
            .map(|r| RecentDto {
                path: display_path(&r.root),
                open: open.contains(&r.root),
                root: r.root,
                name: r.name,
                last_opened: r.last_opened,
            })
            .collect(),
    )
}

#[derive(Deserialize)]
pub struct RecentQuery {
    root: String,
}

pub async fn remove_recent(
    State(state): State<AppState>,
    ApiQuery(q): ApiQuery<RecentQuery>,
) -> StatusCode {
    if let Some(store) = state.workspace.app_state() {
        store.remove_recent(&q.root);
    }
    StatusCode::NO_CONTENT
}

/// UI state of every listed collection (open or unavailable) that has some.
pub async fn all_ui_state(State(state): State<AppState>) -> Json<BTreeMap<String, UiStateDto>> {
    let Some(store) = state.workspace.app_state() else {
        return Json(BTreeMap::new());
    };
    let ids = state
        .workspace
        .list()
        .iter()
        .map(|o| (o.id.clone(), o.canonical_root.clone()))
        .chain(
            state
                .workspace
                .unavailable()
                .into_iter()
                .map(|u| (u.id, u.root)),
        )
        .collect::<Vec<_>>();
    Json(
        ids.into_iter()
            .filter_map(|(id, root)| store.ui(&root).map(|ui| (id.0, ui.into())))
            .collect(),
    )
}

pub async fn put_ui_state(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(ui): ApiJson<UiStateDto>,
) -> Result<StatusCode, ApiError> {
    let root = state
        .workspace
        .canonical_root(&CollectionId(id.clone()))
        .ok_or_else(|| ApiError::not_found(format!("collection `{id}` is not open")))?;
    let ui: plover_core::app_state::UiState = ui.into();
    ui.validate()
        .map_err(|m| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", m))?;
    // Without a store the layout is accepted and not kept.
    if let Some(store) = state.workspace.app_state() {
        store
            .set_ui(&root, ui)
            .map_err(|m| ApiError::new(StatusCode::BAD_REQUEST, "bad_request", m))?;
    }
    Ok(StatusCode::NO_CONTENT)
}
