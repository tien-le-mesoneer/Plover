use axum::Json;
use axum::extract::State;
use base64::Engine;
use plover_core::executor::ResponseSummary;
use plover_core::request::HttpRequest;
use plover_core::runner::{SendInput, send_one};
use plover_core::store::{self, ContentHash};
use serde::Deserialize;

use super::dto::{
    HttpRequestDto, ResponseDto, SaveRequestBody, SavedRequestDto, SendBody, SendResultDto,
    StoredRequestDto, outcome_lists,
};
use super::extract::{ApiJson, ApiPath, ApiQuery};
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
    ApiPath(id): ApiPath<String>,
    ApiQuery(q): ApiQuery<ItemQuery>,
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
    ApiPath(id): ApiPath<String>,
    ApiQuery(q): ApiQuery<ItemQuery>,
    ApiJson(body): ApiJson<SaveRequestBody>,
) -> Result<Json<SavedRequestDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    let request = HttpRequest::from(body.request);
    let saved = store::save_request(&open.root(), &q.item, &request, &ContentHash(body.hash))?;
    open.reload()?;
    // The request as it reads back from disk, so the UI's "saved" state matches the file.
    Ok(Json(SavedRequestDto {
        request: HttpRequestDto::from(&saved.value),
        hash: saved.hash.0,
    }))
}

pub async fn send(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<SendBody>,
) -> Result<Json<SendResultDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    let draft = body.draft.map(HttpRequest::from);
    let sent = send_one(
        &open,
        SendInput {
            item_id: Some(&body.item),
            draft: draft.as_ref(),
            env: body.env.as_deref(),
        },
    )
    .await?;
    let (tests, asserts, console, script_error) = outcome_lists(&sent.outcome);
    Ok(Json(SendResultDto {
        response: sent.response.map(response_dto),
        tests,
        asserts,
        console,
        script_error,
        scripted_requests: sent
            .outcome
            .scripted_requests
            .iter()
            .map(Into::into)
            .collect(),
        skipped: sent.skipped,
    }))
}

pub fn response_dto(res: ResponseSummary) -> ResponseDto {
    let mut body = res.body;
    let truncated = res.truncated || body.len() > MAX_INLINE_BODY;
    body.truncate(MAX_INLINE_BODY);
    let (body_text, body_base64) = match String::from_utf8(body) {
        Ok(text) => (Some(text), None),
        Err(e) => {
            let utf8 = e.utf8_error();
            // A cut through the last character leaves an incomplete sequence at
            // the end (`error_len() == None`): keep the valid text before it.
            if truncated && utf8.error_len().is_none() {
                let mut bytes = e.into_bytes();
                bytes.truncate(utf8.valid_up_to());
                (String::from_utf8(bytes).ok(), None)
            } else {
                (
                    None,
                    Some(base64::engine::general_purpose::STANDARD.encode(e.as_bytes())),
                )
            }
        }
    };
    ResponseDto {
        status: res.status,
        headers: res.headers,
        elapsed_ms: u64::try_from(res.elapsed.as_millis()).unwrap_or(u64::MAX),
        size: res.size,
        size_complete: !res.truncated,
        body_text,
        body_base64,
        truncated,
    }
}
