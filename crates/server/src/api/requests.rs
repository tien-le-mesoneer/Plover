use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use base64::Engine;
use brs_core::executor::ResponseSummary;
use brs_core::request::HttpRequest;
use brs_core::runner::{SendInput, send_one};
use brs_core::store::{self, ContentHash};
use serde::Deserialize;

use super::dto::{
    HashDto, HttpRequestDto, ResponseDto, SaveRequestBody, SendBody, StoredRequestDto,
};
use super::open_collection;
use crate::error::ApiError;
use crate::state::AppState;

/// Response bodies larger than this are cut before being sent to the UI.
pub const MAX_INLINE_BODY: usize = 10 * 1024 * 1024;

#[derive(Deserialize)]
pub struct ItemQuery {
    item: String,
}

pub async fn read(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<ItemQuery>,
) -> Result<Json<StoredRequestDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    let stored = store::read_request(&open.root(), &q.item)?;
    Ok(Json(StoredRequestDto {
        request: HttpRequestDto::from(&stored.value),
        hash: stored.hash.0,
    }))
}

pub async fn save(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<ItemQuery>,
    Json(body): Json<SaveRequestBody>,
) -> Result<Json<HashDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    let request = HttpRequest::from(body.request);
    let saved = store::save_request(&open.root(), &q.item, &request, &ContentHash(body.hash))?;
    open.reload()?;
    Ok(Json(HashDto { hash: saved.hash.0 }))
}

pub async fn send(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<SendBody>,
) -> Result<Json<ResponseDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    let draft = body.draft.map(HttpRequest::from);
    let response = send_one(
        &open,
        SendInput {
            item_id: &body.item,
            draft: draft.as_ref(),
            env: body.env.as_deref(),
        },
    )
    .await?;
    Ok(Json(response_dto(response)))
}

pub fn response_dto(res: ResponseSummary) -> ResponseDto {
    let truncated = res.body.len() > MAX_INLINE_BODY;
    let mut body = res.body;
    body.truncate(MAX_INLINE_BODY);
    let (body_text, body_base64) = match String::from_utf8(body) {
        Ok(text) => (Some(text), None),
        Err(e) => (
            None,
            Some(base64::engine::general_purpose::STANDARD.encode(e.as_bytes())),
        ),
    };
    ResponseDto {
        status: res.status,
        headers: res.headers,
        elapsed_ms: u64::try_from(res.elapsed.as_millis()).unwrap_or(u64::MAX),
        size: res.size,
        body_text,
        body_base64,
        truncated,
    }
}
