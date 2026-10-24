//! `GET /api/ws`: collection runs streamed as `RunEventDto`s. One active run
//! per socket; cancelling or closing the socket drops the run future, which
//! aborts the request in flight. Every run is recorded, masked and saved to the run
//! history when it finishes or is cancelled.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use plover_core::data_file;
use plover_core::masking::secret_strings;
use plover_core::run_record::{
    ResultRecord, ResultStatus, RunEnd, RunHeader, RunId, RunRecord, RunRecorder, RunSource,
};
use plover_core::runner::{
    Iterations, Persist, RunEvent, RunOptions, RunReport, RunVars, run_with_state, secret_values,
};
use plover_core::store;
use plover_core::workspace::{CollectionId, OpenCollection};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use ts_rs::TS;

use super::collections::item_id;
use super::dto::{
    AssertResultDto, ConsoleLineDto, DataFileDto, RunReportDto, ScriptErrorDto, TestResultDto,
    outcome_lists,
};
use crate::security::TOKEN_PROTOCOL_PREFIX;
use crate::state::{AppState, LiveRun};

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
        /// Run once per row of this file (`Run with data…`).
        #[serde(default)]
        #[ts(optional)]
        data: Option<DataFileDto>,
        /// Run this many times; a data file sets the count to its rows instead.
        #[serde(default)]
        #[ts(optional)]
        iteration_count: Option<u32>,
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
    /// The first event of every run: `recordId` names it in the run history and in
    /// `GET /collections/{id}/runs/{recordId}/results/{index}`.
    Accepted {
        run_id: String,
        record_id: String,
    },
    /// Before each iteration of a data-driven or repeated run (not sent for a plain run);
    /// `data` is the row, masked (`{}` without a data file).
    IterationStarted {
        run_id: String,
        index: u32,
        total: u32,
        #[ts(type = "Record<string, unknown>")]
        data: serde_json::Value,
    },
    Started {
        run_id: String,
        /// Zero-based iteration (also on `passed`, `failed`, `skipped`).
        iteration: u32,
        item: String,
        name: String,
        method: String,
        url: String,
    },
    Passed {
        run_id: String,
        /// The result's position in the run (a request run twice has two).
        index: u32,
        iteration: u32,
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
        index: u32,
        iteration: u32,
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
        index: u32,
        iteration: u32,
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
    /// `error` codes: `bad_message`, `not_found`, `bad_item`, `bad_data`, `run_in_progress`,
    /// `run_failed`.
    Error {
        run_id: Option<String>,
        error: String,
        message: String,
    },
}

pub async fn upgrade(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let protocol = format!("{TOKEN_PROTOCOL_PREFIX}{}", state.token);
    // A `start` message can carry a data file of up to 16 MiB; larger files are answered
    // with `bad_data` instead of a dropped connection.
    let limit = data_file::MAX_DATA_FILE + (1 << 20);
    ws.protocols([protocol])
        .max_message_size(limit)
        .max_frame_size(limit)
        .on_upgrade(move |socket| handle(socket, state))
}

struct ActiveRun {
    id: String,
    record_id: String,
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
                    Ok(ClientMessage::Start { run_id, collection_id, item, env, data, iteration_count }) => {
                        if active.as_ref().is_some_and(|a| !a.task.is_finished()) {
                            let _ = tx.send(error(Some(run_id), "run_in_progress", "a run is already active on this socket"));
                            continue;
                        }
                        let iterations = match iterations(data, iteration_count) {
                            Ok(iterations) => iterations,
                            Err(message) => {
                                let _ = tx.send(error(Some(run_id), "bad_data", &message));
                                continue;
                            }
                        };
                        active = start(&state, run_id, collection_id, item, env, iterations, tx.clone());
                    }
                    Ok(ClientMessage::Cancel { run_id }) => {
                        if let Some(run) = active.take_if(|a| a.id == run_id) {
                            run.task.abort();
                            save_cancelled(&state, &run.record_id).await;
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
        save_cancelled(&state, &run.record_id).await;
    }
}

/// Saves an aborted run with the results it had (masked with the secrets it started with
/// and every credential header seen).
async fn save_cancelled(state: &AppState, record_id: &str) {
    let record = {
        let mut live = state.live.lock().unwrap_or_else(|e| e.into_inner());
        let record = match live.remove(record_id) {
            Some(LiveRun::Running(recorder)) => Arc::new(recorder.finish(RunEnd::Cancelled {
                end_secrets: Vec::new(),
            })),
            // The run finished while it was cancelled; save what it finished with.
            Some(LiveRun::Done(record)) => record,
            None => return,
        };
        live.insert(record_id.to_owned(), LiveRun::Done(record.clone()));
        record
    };
    save(state, record).await;
    remove_live(state, record_id);
}

/// Writes a run to history off the async workers. A failure only warns.
async fn save(state: &AppState, record: Arc<RunRecord>) {
    let history = state.history.clone();
    let saved = tokio::task::spawn_blocking(move || history.save(&record)).await;
    match saved {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!("cannot save run history: {e}"),
        Err(e) => tracing::warn!("cannot save run history: {e}"),
    }
}

fn remove_live(state: &AppState, record_id: &str) {
    state
        .live
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(record_id);
}

fn error(run_id: Option<String>, code: &str, message: &str) -> RunEventDto {
    RunEventDto::Error {
        run_id,
        error: code.to_owned(),
        message: message.to_owned(),
    }
}

/// The run's iterations and the data-file credential values to mask.
struct DataRun {
    iterations: Iterations,
    secrets: Vec<String>,
}

/// A data file wins over a count (Bruno CLI); a bad file is an error and nothing runs.
fn iterations(data: Option<DataFileDto>, count: Option<u32>) -> Result<DataRun, String> {
    match (data, count) {
        (Some(file), _) => {
            let rows = data_file::parse(file.format.into(), &file.text)
                .map_err(|e| e.with_name("data file"))?;
            Ok(DataRun {
                secrets: data_file::credential_values(&rows),
                iterations: Iterations::from_rows(rows),
            })
        }
        (None, Some(0)) => Err("the iteration count must be at least 1".to_owned()),
        (None, Some(n)) => Ok(DataRun {
            iterations: Iterations::repeat(n as usize),
            secrets: Vec::new(),
        }),
        (None, None) => Ok(DataRun {
            iterations: Iterations::default(),
            secrets: Vec::new(),
        }),
    }
}

fn start(
    state: &AppState,
    run_id: String,
    collection_id: String,
    item: String,
    env: Option<String>,
    data: DataRun,
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
    let record_id = RunId::new(std::time::SystemTime::now());
    let task = tokio::spawn(run_task(
        state.clone(),
        open,
        Target {
            root,
            path: target,
            item,
        },
        record_id.clone(),
        run_id,
        Settings { env, data },
        tx,
    ));
    Some(ActiveRun {
        id,
        record_id: record_id.0,
        task,
    })
}

/// The environment and iterations a run was started with.
struct Settings {
    env: Option<String>,
    data: DataRun,
}

struct Target {
    root: PathBuf,
    path: PathBuf,
    /// The folder or request ID that was started; `""` for the collection.
    item: String,
}

async fn run_task(
    app: AppState,
    open: Arc<OpenCollection>,
    target: Target,
    record_id: RunId,
    run_id: String,
    settings: Settings,
    tx: mpsc::UnboundedSender<RunEventDto>,
) {
    let Settings { env, data } = settings;
    let Target {
        root,
        path: target,
        item,
    } = target;
    let collection = open.collection();
    let opts = RunOptions {
        env_overrides: env
            .as_deref()
            .map(|e| open.env_overrides(e))
            .unwrap_or_default(),
        env,
        // The Bruno app writes script changes to the environment file.
        persist: Persist::Environment,
        iterations: data.iterations,
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
    let canonical = root.canonicalize().unwrap_or_else(|_| root.clone());
    let header = RunHeader {
        id: record_id.clone(),
        source: RunSource::App,
        collection_root: canonical.to_string_lossy().into_owned(),
        collection_name: collection.config.name.clone(),
        target: if item.is_empty() {
            ".".to_owned()
        } else {
            item
        },
        env: opts.env.clone(),
        started_at_ms: plover_core::time::unix_ms(std::time::SystemTime::now()),
    };
    let mut start_secrets = secret_strings(&secret_values(environment, &state.environment));
    start_secrets.extend(data.secrets);
    let record_key = record_id.0.clone();
    app.live.lock().unwrap_or_else(|e| e.into_inner()).insert(
        record_key.clone(),
        LiveRun::Running(Box::new(RunRecorder::new(&root, header, start_secrets))),
    );
    let events = tx.clone();
    let id = run_id.clone();
    let live = app.live.clone();
    let sent_by = open.clone();
    let mut accepted = false;
    let mut on_event = move |event: &RunEvent| {
        if let RunEvent::Passed { outcome, .. } | RunEvent::Failed { outcome, .. } = event
            && let Some(detail) = &outcome.request
        {
            sent_by.note_sent_credentials(plover_core::masking::credential_values(&detail.headers));
        }
        if !accepted {
            accepted = true;
            let _ = events.send(RunEventDto::Accepted {
                run_id: id.clone(),
                record_id: record_key.clone(),
            });
        }
        // `Finished` is sent once the run is masked and saved.
        if matches!(event, RunEvent::Finished { .. }) {
            return;
        }
        let mut live = live.lock().unwrap_or_else(|e| e.into_inner());
        let Some(LiveRun::Running(recorder)) = live.get_mut(&record_key) else {
            return;
        };
        let index = recorder.event(event);
        match event {
            // A plain run (one iteration, no data file) sends no iteration events, so its
            // stream is unchanged.
            RunEvent::IterationStarted { total: 1, data, .. } if data.is_empty() => {}
            RunEvent::IterationStarted { index, total, .. } => {
                let data = recorder
                    .masked_iteration(*index)
                    .map(|i| row_json(&i.data))
                    .unwrap_or_else(|| serde_json::json!({}));
                let _ = events.send(RunEventDto::IterationStarted {
                    run_id: id.clone(),
                    index: u32_of(*index),
                    total: u32_of(*total),
                    data,
                });
            }
            RunEvent::IterationFinished { .. } => {}
            _ => {
                let _ = events.send(event_dto(&root, &id, index, event));
            }
        }
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
    let end_secrets = secret_strings(&secret_values(environment, &state.environment));
    match result {
        Ok(report) => {
            let record = {
                let mut live = app.live.lock().unwrap_or_else(|e| e.into_inner());
                match live.remove(&record_id.0) {
                    Some(LiveRun::Running(recorder)) => {
                        let record = Arc::new(recorder.finish(RunEnd::Finished {
                            report,
                            end_secrets,
                        }));
                        live.insert(record_id.0.clone(), LiveRun::Done(record.clone()));
                        Some(record)
                    }
                    _ => None,
                }
            };
            if let Some(record) = record {
                save(&app, record).await;
            }
            let _ = tx.send(RunEventDto::Finished {
                run_id: run_id.clone(),
                report: report_dto(&report),
            });
            remove_live(&app, &record_id.0);
            open.record_runtime_changes(&runtime_before, &state.runtime);
            if let Some(env) = &opts.env {
                // Other changes are in the environment file now; secrets stay in memory.
                open.record_env_changes(
                    env,
                    &secret_values(environment, &environment_before),
                    &secret_values(environment, &state.environment),
                );
                if environment_before != state.environment
                    && let Err(e) = open.reload()
                {
                    tracing::warn!("cannot reload the collection after a run: {e}");
                }
            }
        }
        Err(e) => {
            // Nothing ran (unknown environment or target): nothing to keep.
            remove_live(&app, &record_id.0);
            let _ = tx.send(error(Some(run_id), "run_failed", &e.to_string()));
        }
    }
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

pub fn row_json(row: &plover_core::vars::ValueMap) -> serde_json::Value {
    serde_json::Value::Object(row.clone().into_iter().collect())
}

fn event_dto(root: &Path, run_id: &str, index: Option<usize>, event: &RunEvent) -> RunEventDto {
    let run_id = run_id.to_owned();
    let index = index.map_or(0, |i| u32::try_from(i).unwrap_or(u32::MAX));
    match event {
        RunEvent::Started {
            iteration,
            path,
            name,
            method,
            url,
        } => RunEventDto::Started {
            run_id,
            iteration: u32_of(*iteration),
            item: item_id(root, path),
            name: name.clone(),
            method: method.clone(),
            url: url.clone(),
        },
        RunEvent::Passed {
            iteration,
            path,
            outcome,
        } => {
            let (tests, asserts, console, _) = outcome_lists(outcome);
            RunEventDto::Passed {
                run_id,
                index,
                iteration: u32_of(*iteration),
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
            iteration,
            path,
            error,
            outcome,
            ..
        } => {
            let (tests, asserts, console, script_error) = outcome_lists(outcome);
            RunEventDto::Failed {
                run_id,
                index,
                iteration: u32_of(*iteration),
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
        RunEvent::Skipped {
            iteration,
            path,
            reason,
            ..
        } => RunEventDto::Skipped {
            run_id,
            index,
            iteration: u32_of(*iteration),
            item: item_id(root, path),
            reason: reason.clone(),
        },
        RunEvent::Finished { report } => RunEventDto::Finished {
            run_id,
            report: report_dto(report),
        },
        RunEvent::IterationStarted { .. } | RunEvent::IterationFinished { .. } => {
            unreachable!("iteration events are sent by the run task")
        }
    }
}

fn millis(d: &std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// A saved result as the events a live run sent for it (`Started` only when it was sent
/// or failed after starting, as live).
pub fn result_events(run_id: &str, r: &ResultRecord) -> Vec<RunEventDto> {
    let run_id = run_id.to_owned();
    let index = u32::try_from(r.index).unwrap_or(u32::MAX);
    let iteration = u32_of(r.iteration_index);
    let o = &r.outcome;
    let (tests, asserts, console, script_error) = outcome_lists(o);
    let mut events = Vec::new();
    if !r.method.is_empty() {
        events.push(RunEventDto::Started {
            run_id: run_id.clone(),
            iteration,
            item: r.file.clone(),
            name: r.name.clone(),
            method: r.method.clone(),
            url: r.url.clone(),
        });
    }
    events.push(match r.status {
        ResultStatus::Pass => RunEventDto::Passed {
            run_id,
            index,
            iteration,
            item: r.file.clone(),
            status: o.status.unwrap_or_default(),
            elapsed_ms: millis(&o.elapsed),
            size: o.size,
            tests,
            asserts,
            console,
        },
        ResultStatus::Fail | ResultStatus::Error => RunEventDto::Failed {
            run_id,
            index,
            iteration,
            item: r.file.clone(),
            error: r.error.clone().unwrap_or_default(),
            status: o.status,
            elapsed_ms: o.status.map(|_| millis(&o.elapsed)),
            tests,
            asserts,
            console,
            script_error,
        },
        ResultStatus::Skipped => RunEventDto::Skipped {
            run_id,
            index,
            iteration,
            item: r.file.clone(),
            reason: r.skip_reason.clone().unwrap_or_default(),
        },
    });
    events
}

pub fn report_dto(r: &RunReport) -> RunReportDto {
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
        iterations: count(r.iterations.max(1)),
    }
}
