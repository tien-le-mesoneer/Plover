//! Send history: `GET /sends?collection=`, `GET /sends/{id}` and
//! `DELETE /sends?collection=`. Records are masked before they are written, so nothing
//! here can hand out a secret.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use plover_core::run_history::HistoryError;
use plover_core::run_record::RunId;
use plover_core::send_history::SendMeta;
use plover_core::workspace::CollectionId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::dto::RunResultDetailDto;
use super::extract::{ApiPath, ApiQuery};
use super::history::detail_dto;
use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Clone, Deserialize)]
pub struct SendsQuery {
    /// A collection ID; all collections when absent.
    #[serde(default)]
    pub collection: Option<String>,
}

/// One past send, as the History list shows it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SendMetaDto {
    pub id: String,
    #[ts(type = "number")]
    pub sent_at_ms: u64,
    /// The collection's ID (it may not be open any more).
    pub collection_id: String,
    pub collection_name: String,
    /// The request file; `null` for a scratch request.
    pub item: Option<String>,
    pub env: Option<String>,
    pub method: String,
    pub name: String,
    pub url: String,
    /// `pass`, `fail`, `error` or `skipped`.
    pub status: String,
    pub http_status: Option<u16>,
    #[ts(type = "number")]
    pub duration_ms: u64,
}

/// `GET /sends/{id}`: a past send with its request and response, masked.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SendRecordDto {
    pub meta: SendMetaDto,
    pub detail: RunResultDetailDto,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RemovedDto {
    pub removed: u32,
}

fn meta_dto(m: &SendMeta) -> SendMetaDto {
    SendMetaDto {
        id: m.header.id.0.clone(),
        sent_at_ms: m.header.sent_at_ms,
        collection_id: CollectionId::of_canonical(&m.header.collection_root).0,
        collection_name: m.header.collection_name.clone(),
        item: m.header.item.clone(),
        env: m.header.env.clone(),
        method: m.method.clone(),
        name: m.name.clone(),
        url: m.url.clone(),
        status: m.status.as_str().to_owned(),
        http_status: m.http_status,
        duration_ms: m.duration_ms,
    }
}

fn io_error(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "io_error", e.to_string())
}

pub async fn list(
    State(state): State<AppState>,
    ApiQuery(q): ApiQuery<SendsQuery>,
) -> Result<Json<Vec<SendMetaDto>>, ApiError> {
    let history = state.send_history.clone();
    let metas =
        tokio::task::spawn_blocking(move || history.list(q.collection.map(CollectionId).as_ref()))
            .await
            .map_err(io_error)?;
    Ok(Json(metas.iter().map(meta_dto).collect()))
}

pub async fn get(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
) -> Result<Json<SendRecordDto>, ApiError> {
    let run_id =
        RunId::parse(&id).ok_or_else(|| ApiError::not_found(format!("send {id} not found")))?;
    let history = state.send_history.clone();
    let record = tokio::task::spawn_blocking(move || history.load(&run_id))
        .await
        .map_err(io_error)?
        .map_err(|e| match e {
            HistoryError::NotFound(_) => ApiError::not_found(format!("send {id} not found")),
            other => io_error(other),
        })?;
    Ok(Json(SendRecordDto {
        meta: meta_dto(&SendMeta::of(&record)),
        detail: detail_dto(&record.result),
    }))
}

pub async fn clear(
    State(state): State<AppState>,
    ApiQuery(q): ApiQuery<SendsQuery>,
) -> Result<Json<RemovedDto>, ApiError> {
    let history = state.send_history.clone();
    let removed =
        tokio::task::spawn_blocking(move || history.clear(q.collection.map(CollectionId).as_ref()))
            .await
            .map_err(io_error)?
            .map_err(io_error)?;
    Ok(Json(RemovedDto {
        removed: u32::try_from(removed).unwrap_or(u32::MAX),
    }))
}
