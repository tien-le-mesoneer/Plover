//! Run history: `GET /collections/{id}/runs`, `GET …/runs/{runId}`,
//! `GET …/runs/{runId}/results/{index}` and `DELETE /collections/{id}/runs`. Runs still in
//! memory (in progress or being saved) are answered from there, others from the history.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use plover_core::run_history::HistoryError;
use plover_core::run_record::{BodyDetail, ResultRecord, RunId, RunOutcome, RunRecord};
use plover_core::workspace::CollectionId;

use super::dto::{
    ResponseDto, RunDto, RunMetaDto, RunResultDetailDto, SendResultDto, outcome_lists,
};
use super::extract::ApiPath;
use super::open_collection;
use super::runs::{RunEventDto, report_dto, result_events};
use crate::error::ApiError;
use crate::state::{AppState, LiveRun};

pub async fn list(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
) -> Result<Json<Vec<RunMetaDto>>, ApiError> {
    open_collection(&state, &id)?;
    let history = state.history.clone();
    let metas = tokio::task::spawn_blocking(move || history.list(&CollectionId(id)))
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "io_error", e.to_string()))?;
    Ok(Json(
        metas
            .iter()
            .map(|m| meta_dto(&m.header, m.outcome, &m.report))
            .collect(),
    ))
}

pub async fn get(
    State(state): State<AppState>,
    ApiPath((id, run_id)): ApiPath<(String, String)>,
) -> Result<Json<RunDto>, ApiError> {
    let record = load(&state, &id, &run_id).await?;
    Ok(Json(run_dto(&record)))
}

pub async fn result(
    State(state): State<AppState>,
    ApiPath((id, run_id, index)): ApiPath<(String, String, usize)>,
) -> Result<Json<RunResultDetailDto>, ApiError> {
    let not_found = || ApiError::not_found(format!("run {run_id} has no result {index}"));
    let result = match live_result(&state, &id, &run_id, index) {
        Some(found) => found.ok_or_else(not_found)?,
        None => load(&state, &id, &run_id)
            .await?
            .results
            .get(index)
            .cloned()
            .ok_or_else(not_found)?,
    };
    Ok(Json(detail_dto(&result)))
}

pub async fn clear(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
) -> Result<StatusCode, ApiError> {
    open_collection(&state, &id)?;
    let history = state.history.clone();
    tokio::task::spawn_blocking(move || history.clear(&CollectionId(id)))
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "io_error", e.to_string()))?
        .map_err(history_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn parse_id(run_id: &str) -> Result<RunId, ApiError> {
    RunId::parse(run_id).ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_run_id",
            format!("`{run_id}` is not a run ID"),
        )
    })
}

/// `Some` when the run is in memory: `Some(None)` when it has no such result.
fn live_result(
    state: &AppState,
    collection: &str,
    run_id: &str,
    index: usize,
) -> Option<Option<ResultRecord>> {
    let live = state.live.lock().unwrap_or_else(|e| e.into_inner());
    match live.get(run_id)? {
        LiveRun::Running(recorder) => {
            (CollectionId::of_canonical(&recorder.header().collection_root).0 == collection)
                .then(|| recorder.masked_result(index))
        }
        LiveRun::Done(record) => (CollectionId::of_canonical(&record.header.collection_root).0
            == collection)
            .then(|| record.results.get(index).cloned()),
    }
}

/// A finished run: from memory while it is being saved, else from the history.
async fn load(
    state: &AppState,
    collection: &str,
    run_id: &str,
) -> Result<Arc<RunRecord>, ApiError> {
    open_collection(state, collection)?;
    let id = parse_id(run_id)?;
    if let Some(LiveRun::Done(record)) = state
        .live
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(run_id)
        && CollectionId::of_canonical(&record.header.collection_root).0 == collection
    {
        return Ok(record.clone());
    }
    let history = state.history.clone();
    let key = CollectionId(collection.to_owned());
    tokio::task::spawn_blocking(move || history.load(&key, &id))
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "io_error", e.to_string()))?
        .map(Arc::new)
        .map_err(history_error)
}

fn history_error(e: HistoryError) -> ApiError {
    match e {
        HistoryError::NotFound(_) => ApiError::not_found(e.to_string()),
        HistoryError::Unreadable { .. } => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "run_unreadable",
            e.to_string(),
        ),
        HistoryError::Io(_) => {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "io_error", e.to_string())
        }
    }
}

fn meta_dto(
    h: &plover_core::run_record::RunHeader,
    outcome: RunOutcome,
    report: &plover_core::runner::RunReport,
) -> RunMetaDto {
    RunMetaDto {
        record_id: h.id.to_string(),
        source: h.source.as_str().to_owned(),
        target: h.target.clone(),
        env: h.env.clone(),
        started_at_ms: h.started_at_ms,
        outcome: outcome.as_str().to_owned(),
        report: report_dto(report),
    }
}

fn run_dto(record: &RunRecord) -> RunDto {
    let h = &record.header;
    let run_id = h.id.to_string();
    let mut events: Vec<RunEventDto> = record
        .results
        .iter()
        .flat_map(|r| result_events(&run_id, r))
        .collect();
    events.push(match record.outcome {
        RunOutcome::Completed => RunEventDto::Finished {
            run_id: run_id.clone(),
            report: report_dto(&record.report),
        },
        RunOutcome::Cancelled => RunEventDto::Cancelled {
            run_id: run_id.clone(),
        },
    });
    let label = if h.target == "." {
        h.collection_name.clone()
    } else {
        h.target.clone()
    };
    RunDto {
        meta: meta_dto(h, record.outcome, &record.report),
        label,
        events,
    }
}

fn detail_dto(r: &ResultRecord) -> RunResultDetailDto {
    let o = &r.outcome;
    let (tests, asserts, console, script_error) = outcome_lists(o);
    let response = o.response.as_ref().map(|d| {
        let (body_text, body_base64, truncated) = match &d.body {
            BodyDetail::Text {
                text, truncated, ..
            } => (Some(text.clone()), None, *truncated),
            BodyDetail::Base64 {
                data, truncated, ..
            } => (None, Some(data.clone()), *truncated),
            BodyDetail::Omitted { .. } => (None, None, true),
            _ => (Some(String::new()), None, false),
        };
        ResponseDto {
            status: d.status,
            headers: d.headers.clone(),
            elapsed_ms: u64::try_from(o.elapsed.as_millis()).unwrap_or(u64::MAX),
            size: d.size,
            size_complete: d.size_complete,
            body_text,
            body_base64,
            truncated,
        }
    });
    RunResultDetailDto {
        index: u32::try_from(r.index).unwrap_or(u32::MAX),
        item: r.file.clone(),
        name: r.name.clone(),
        status: r.status.as_str().to_owned(),
        error: r.error.clone(),
        skip_reason: r.skip_reason.clone(),
        request: o.request.as_ref().map(Into::into),
        response_body: o.response.as_ref().map(|d| (&d.body).into()),
        result: SendResultDto {
            response,
            tests,
            asserts,
            console,
            script_error,
            scripted_requests: o.scripted_requests.iter().map(Into::into).collect(),
            skipped: r.skip_reason.clone(),
        },
    }
}
