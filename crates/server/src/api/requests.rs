use std::sync::{Arc, PoisonError};

use axum::Json;
use axum::extract::State;
use base64::Engine;
use plover_core::executor::ResponseSummary;
use plover_core::masking::credential_values;
use plover_core::request::HttpRequest;
use plover_core::run_record::{ResultStatus, RunId};
use plover_core::runner::{RequestOutcome, SendError, SendInput, send_one};
use plover_core::send_history::{SendHeader, SendRecord, send_result};
use plover_core::store::{self, ContentHash};
use plover_core::workspace::OpenCollection;
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
                // A cancelled send is not recorded.
                return Err(ApiError::new(StatusCode::CONFLICT, "cancelled", "Request cancelled"));
            }
            sent = sending => sent,
        },
        None => sending.await,
    };
    let recorded = Recorded {
        state: &state,
        open: &open,
        item: body.item.as_deref(),
        draft: draft.as_ref(),
        env: body.env.as_deref(),
    };
    let sent = match sent {
        Ok(sent) => sent,
        Err(e @ SendError::Exec(_)) => {
            // The transport failed: there is no outcome, but the attempt is history.
            recorded
                .save(&RequestOutcome::default(), Some(e.to_string()), None)
                .await;
            return Err(e.into());
        }
        Err(e) => return Err(e.into()),
    };
    if let Some(detail) = &sent.outcome.request {
        open.note_sent_credentials(credential_values(&detail.headers));
    }
    let history_error = recorded
        .save(&sent.outcome, None, sent.skipped.clone())
        .await;
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
        history_error,
    }))
}

/// What a send needs to be saved to history.
struct Recorded<'a> {
    state: &'a AppState,
    open: &'a OpenCollection,
    item: Option<&'a str>,
    draft: Option<&'a HttpRequest>,
    env: Option<&'a str>,
}

impl Recorded<'_> {
    /// Saves the send, masked with the environment's secrets and the credential header
    /// values it sent; returns why it could not be saved. Never fails the send.
    async fn save(
        &self,
        outcome: &RequestOutcome,
        error: Option<String>,
        skipped: Option<String>,
    ) -> Option<String> {
        let root = self.open.root();
        let canonical = root.canonicalize().unwrap_or(root);
        let name = self.draft.map(|d| d.name.clone()).unwrap_or_else(|| {
            let item = self.item.unwrap_or_default();
            std::path::Path::new(item)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let (method, url) = self
            .draft
            .map(|d| (d.method.as_str().to_owned(), d.url.clone()))
            .unwrap_or_default();
        let mut result = send_result(self.item, &name, &method, &url, outcome, error);
        if let Some(reason) = skipped {
            result.status = ResultStatus::Skipped;
            result.error = None;
            result.skip_reason = Some(reason);
        }
        let mut secrets: Vec<String> = self
            .env
            .map(|env| self.open.secrets(env).into_values().collect())
            .unwrap_or_default();
        secrets.extend(self.open.sent_credentials());
        let now = std::time::SystemTime::now();
        let header = SendHeader {
            id: RunId::new(now),
            sent_at_ms: plover_core::time::unix_ms(now),
            collection_root: canonical.to_string_lossy().into_owned(),
            collection_name: self.open.collection().config.name.clone(),
            item: self.item.map(str::to_owned),
            env: self.env.map(str::to_owned),
        };
        let record = SendRecord::masked(header, result, &secrets);
        let history = self.state.send_history.clone();
        match tokio::task::spawn_blocking(move || history.save(&record)).await {
            Ok(Ok(())) => None,
            Ok(Err(e)) => {
                tracing::warn!("cannot save send history: {e}");
                Some(e.to_string())
            }
            Err(e) => Some(e.to_string()),
        }
    }
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
