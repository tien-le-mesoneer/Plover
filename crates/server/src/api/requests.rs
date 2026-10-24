use std::sync::{Arc, PoisonError};

use axum::Json;
use axum::extract::State;
use base64::Engine;
use plover_core::executor::ResponseSummary;
use plover_core::request::HttpRequest;
use plover_core::runner::{SendInput, send_one};
use plover_core::store::{self, ContentHash};
use serde::Deserialize;
use tokio::sync::Notify;

use axum::http::StatusCode;

use super::dto::{
    CreateRequestBody, CreatedRequestDto, HttpRequestDto, ResponseDto, SaveRequestBody,
    SavedRequestDto, SendBody, SendResultDto, StoredRequestDto, outcome_lists,
};
use super::extract::{ApiJson, ApiPath, ApiQuery};
use super::open_collection;
use crate::error::ApiError;
use crate::state::{AppState, SendRegistry};

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

/// Writes a new request file (`POST …/requests/new`); 409 `exists` if the name is taken.
pub async fn create(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<CreateRequestBody>,
) -> Result<Json<CreatedRequestDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    let request = HttpRequest::from(body.request);
    let (item, stored) = store::create_request(&open.root(), &body.folder, &body.name, &request)?;
    open.reload()?;
    Ok(Json(CreatedRequestDto {
        item,
        request: HttpRequestDto::from(&stored.value),
        hash: stored.hash.0,
    }))
}

pub async fn send(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<SendBody>,
) -> Result<Json<SendResultDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    if body.item.is_none() && body.draft.is_none() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_item",
            "a scratch request needs a draft",
        ));
    }
    let registered = match body.send_id {
        Some(send_id) => Some(Registered::new(&state.sends, &id, send_id)?),
        None => None,
    };
    let draft = body.draft.map(HttpRequest::from);
    let sending = send_one(
        &open,
        SendInput {
            item_id: body.item.as_deref(),
            draft: draft.as_ref(),
            env: body.env.as_deref(),
        },
    );
    // Dropping `sending` stops the HTTP request and any script; runtime and environment
    // changes are only applied when a send finishes.
    let sent = match &registered {
        Some(r) => tokio::select! {
            biased;
            () = r.cancelled.notified() => {
                return Err(ApiError::new(StatusCode::CONFLICT, "cancelled", "Request cancelled"));
            }
            sent = sending => sent?,
        },
        None => sending.await?,
    };
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

/// `POST /collections/{id}/send/{send_id}/cancel`: 204 whether or not the send is still
/// running.
pub async fn cancel(
    State(state): State<AppState>,
    ApiPath((id, send_id)): ApiPath<(String, String)>,
) -> Result<StatusCode, ApiError> {
    check_send_id(&send_id)?;
    let sends = state.sends.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(cancelled) = sends.get(&(id, send_id)) {
        cancelled.notify_one();
    }
    Ok(StatusCode::NO_CONTENT)
}

fn check_send_id(send_id: &str) -> Result<(), ApiError> {
    let ok = (1..=64).contains(&send_id.len())
        && send_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_send_id",
            "a send ID is 1-64 letters, digits or `-`",
        ))
    }
}

/// A send's entry in the registry, removed however the send ends (finished, failed,
/// cancelled, or the client went away and the handler was dropped).
struct Registered {
    sends: Arc<SendRegistry>,
    key: (String, String),
    cancelled: Arc<Notify>,
}

impl Registered {
    fn new(sends: &Arc<SendRegistry>, id: &str, send_id: String) -> Result<Registered, ApiError> {
        check_send_id(&send_id)?;
        let key = (id.to_owned(), send_id);
        let cancelled = Arc::new(Notify::new());
        let mut map = sends.lock().unwrap_or_else(PoisonError::into_inner);
        if map.contains_key(&key) {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "duplicate_send",
                "a send with this ID is still running",
            ));
        }
        map.insert(key.clone(), cancelled.clone());
        Ok(Registered {
            sends: sends.clone(),
            key,
            cancelled,
        })
    }
}

impl Drop for Registered {
    fn drop(&mut self) {
        self.sends
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
    }
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
