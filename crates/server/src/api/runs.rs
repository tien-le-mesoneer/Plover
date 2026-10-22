//! `GET /api/ws`: collection runs streamed as `RunEventDto`s. One active run
//! per socket; cancelling or closing the socket drops the run future, which
//! aborts the request in flight.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use brs_core::runner::{RunEvent, RunOptions, RunReport, RunVars, run_with_state};
use brs_core::store;
use brs_core::workspace::{CollectionId, OpenCollection};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use ts_rs::TS;

use super::collections::item_id;
use super::dto::{
    AssertResultDto, ConsoleLineDto, RunReportDto, ScriptErrorDto, TestResultDto, outcome_lists,
};
use crate::security::TOKEN_PROTOCOL_PREFIX;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ClientMessage {
    /// `item` is a folder or request ID; `""` runs the whole collection.
    Start {
        run_id: String,
        collection_id: String,
        item: String,
        env: Option<String>,
    },
    Cancel {
        run_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RunEventDto {
    Started {
        run_id: String,
        item: String,
        name: String,
        method: String,
        url: String,
    },
    Passed {
        run_id: String,
        item: String,
        status: u16,
        #[ts(type = "number")]
        elapsed_ms: u64,
        #[ts(type = "number")]
        size: usize,
        tests: Vec<TestResultDto>,
        asserts: Vec<AssertResultDto>,
        console: Vec<ConsoleLineDto>,
    },
    /// A transport or script error, or a failed test or assert. `status` is set when a
    /// response arrived.
    Failed {
        run_id: String,
        item: String,
        error: String,
        status: Option<u16>,
        #[ts(type = "number | null")]
        elapsed_ms: Option<u64>,
        tests: Vec<TestResultDto>,
        asserts: Vec<AssertResultDto>,
        console: Vec<ConsoleLineDto>,
        script_error: Option<ScriptErrorDto>,
    },
    Skipped {
        run_id: String,
        item: String,
        reason: String,
    },
    Finished {
        run_id: String,
        report: RunReportDto,
    },
    Cancelled {
        run_id: String,
    },
    /// `error` codes: `bad_message`, `not_found`, `bad_item`, `run_in_progress`, `run_failed`.
    Error {
        run_id: Option<String>,
        error: String,
        message: String,
    },
}

pub async fn upgrade(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let protocol = format!("{TOKEN_PROTOCOL_PREFIX}{}", state.token);
    ws.protocols([protocol])
        .on_upgrade(move |socket| handle(socket, state))
}

struct ActiveRun {
    id: String,
    task: JoinHandle<()>,
}

async fn handle(mut socket: WebSocket, state: AppState) {
    let (tx, mut rx) = mpsc::unbounded_channel::<RunEventDto>();
    let mut active: Option<ActiveRun> = None;
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let text = match incoming {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(_)) => continue,
                };
                match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(ClientMessage::Start { run_id, collection_id, item, env }) => {
                        if active.as_ref().is_some_and(|a| !a.task.is_finished()) {
                            let _ = tx.send(error(Some(run_id), "run_in_progress", "a run is already active on this socket"));
                            continue;
                        }
                        active = start(&state, run_id, collection_id, item, env, tx.clone());
                    }
                    Ok(ClientMessage::Cancel { run_id }) => {
                        if let Some(run) = active.take_if(|a| a.id == run_id) {
                            run.task.abort();
                            let _ = tx.send(RunEventDto::Cancelled { run_id });
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(error(None, "bad_message", &e.to_string()));
                    }
                }
            }
            Some(event) = rx.recv() => {
                let Ok(json) = serde_json::to_string(&event) else { continue };
                if socket.send(Message::text(json)).await.is_err() {
                    break;
                }
            }
        }
    }
    if let Some(run) = active {
        run.task.abort();
    }
}

fn error(run_id: Option<String>, code: &str, message: &str) -> RunEventDto {
    RunEventDto::Error {
        run_id,
        error: code.to_owned(),
        message: message.to_owned(),
    }
}

fn start(
    state: &AppState,
    run_id: String,
    collection_id: String,
    item: String,
    env: Option<String>,
    tx: mpsc::UnboundedSender<RunEventDto>,
) -> Option<ActiveRun> {
    let Some(open) = state.workspace.get(&CollectionId(collection_id.clone())) else {
        let _ = tx.send(error(
            Some(run_id),
            "not_found",
            &format!("collection `{collection_id}` is not open"),
        ));
        return None;
    };
    let root = open.root();
    let target = if item.is_empty() {
        root.clone()
    } else {
        match store::resolve_item_path(&root, &item) {
            Ok(path) => path,
            Err(e) => {
                let _ = tx.send(error(Some(run_id), "bad_item", &e.to_string()));
                return None;
            }
        }
    };
    let id = run_id.clone();
    let task = tokio::spawn(run_task(open, root, target, run_id, env, tx));
    Some(ActiveRun { id, task })
}

async fn run_task(
    open: Arc<OpenCollection>,
    root: PathBuf,
    target: PathBuf,
    run_id: String,
    env: Option<String>,
    tx: mpsc::UnboundedSender<RunEventDto>,
) {
    let collection = open.collection();
    let opts = RunOptions {
        env_overrides: env
            .as_deref()
            .map(|e| open.env_overrides(e))
            .unwrap_or_default(),
        env,
        ..RunOptions::default()
    };
    let environment = opts.env.as_deref().and_then(|e| collection.environment(e));
    let runtime_before = open.runtime();
    let mut state = RunVars::start(
        &collection,
        environment,
        &opts.env_overrides,
        runtime_before.clone(),
    );
    let environment_before = state.environment.clone();
    let events = tx.clone();
    let id = run_id.clone();
    let mut on_event = move |event: &RunEvent| {
        let _ = events.send(event_dto(&root, &id, event));
    };
    let result = run_with_state(
        &collection,
        &target,
        &opts,
        &open.session,
        &mut state,
        &mut on_event,
    )
    .await;
    match result {
        Ok(_) => {
            open.record_runtime_changes(&runtime_before, &state.runtime);
            if let Some(env) = &opts.env {
                open.record_env_changes(env, &environment_before, &state.environment);
            }
        }
        Err(e) => {
            let _ = tx.send(error(Some(run_id), "run_failed", &e.to_string()));
        }
    }
}

fn event_dto(root: &std::path::Path, run_id: &str, event: &RunEvent) -> RunEventDto {
    let run_id = run_id.to_owned();
    match event {
        RunEvent::Started {
            path,
            name,
            method,
            url,
        } => RunEventDto::Started {
            run_id,
            item: item_id(root, path),
            name: name.clone(),
            method: method.clone(),
            url: url.clone(),
        },
        RunEvent::Passed { path, outcome } => {
            let (tests, asserts, console, _) = outcome_lists(outcome);
            RunEventDto::Passed {
                run_id,
                item: item_id(root, path),
                status: outcome.status.unwrap_or_default(),
                elapsed_ms: millis(&outcome.elapsed),
                size: outcome.size,
                tests,
                asserts,
                console,
            }
        }
        RunEvent::Failed {
            path,
            error,
            outcome,
        } => {
            let (tests, asserts, console, script_error) = outcome_lists(outcome);
            RunEventDto::Failed {
                run_id,
                item: item_id(root, path),
                error: error.clone(),
                status: outcome.status,
                elapsed_ms: outcome.status.map(|_| millis(&outcome.elapsed)),
                tests,
                asserts,
                console,
                script_error,
            }
        }
        RunEvent::Skipped { path, reason, .. } => RunEventDto::Skipped {
            run_id,
            item: item_id(root, path),
            reason: reason.clone(),
        },
        RunEvent::Finished { report } => RunEventDto::Finished {
            run_id,
            report: report_dto(report),
        },
    }
}

fn millis(d: &std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn report_dto(r: &RunReport) -> RunReportDto {
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    RunReportDto {
        passed: count(r.passed),
        failed: count(r.failed),
        skipped: count(r.skipped),
        tests_passed: count(r.tests_passed),
        tests_failed: count(r.tests_failed),
        asserts_passed: count(r.asserts_passed),
        asserts_failed: count(r.asserts_failed),
        elapsed_ms: millis(&r.elapsed),
    }
}
