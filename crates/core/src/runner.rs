//! Runs the requests under a target in Bruno's order and reports each one as a `RunEvent`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::bru::model::RequestFile;
use crate::collection::{Collection, Item, RequestRef};
use crate::executor::{ExecError, ExecOptions, ResponseSummary, Session};
use crate::request::HttpRequest;
use crate::resolve::{ResolveContext, ResolveError, ResolvedRequest, resolve};
use crate::store::{self, StoreError};
use crate::vars::VarMap;
use crate::workspace::OpenCollection;

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub env: Option<String>,
    pub env_overrides: VarMap,
    pub exec: ExecOptions,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunEvent {
    Started {
        path: PathBuf,
        name: String,
        method: String,
        url: String,
    },
    Passed {
        path: PathBuf,
        status: u16,
        elapsed: Duration,
        size: usize,
    },
    Failed {
        path: PathBuf,
        error: String,
    },
    Skipped {
        path: PathBuf,
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunReport {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub elapsed: Duration,
}

/// Setup problems only; per-request problems are reported as events.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("environment `{0}` not found")]
    UnknownEnvironment(String),
    #[error("{0} is not a request or folder in this collection")]
    TargetNotFound(PathBuf),
    #[error(transparent)]
    Session(#[from] ExecError),
}

pub async fn run(
    collection: &Collection,
    target: &Path,
    opts: &RunOptions,
    on_event: &mut (dyn FnMut(&RunEvent) + Send),
) -> Result<RunReport, RunError> {
    let environment = match &opts.env {
        Some(name) => Some(
            collection
                .environment(name)
                .ok_or_else(|| RunError::UnknownEnvironment(name.clone()))?,
        ),
        None => None,
    };
    let absolute = std::path::absolute(target).unwrap_or_else(|_| target.to_path_buf());
    if !absolute.starts_with(&collection.root) || !absolute.exists() {
        return Err(RunError::TargetNotFound(target.to_path_buf()));
    }
    let refs = collection.requests_under(&absolute);
    let session = Session::new(opts.exec.clone())?;
    // Filled by scripts in Phase 3.
    let runtime = VarMap::new();
    let ctx = ResolveContext {
        collection,
        environment,
        env_overrides: &opts.env_overrides,
        runtime: &runtime,
    };

    let start = Instant::now();
    let mut report = RunReport::default();
    for r in refs {
        let path = r.path.to_path_buf();
        let resolved = match prepare(&ctx, &r) {
            Ok(resolved) => resolved,
            Err(NotRunnable::Skip(reason)) => {
                report.skipped += 1;
                on_event(&RunEvent::Skipped { path, reason });
                continue;
            }
            Err(NotRunnable::Fail(e)) => {
                report.failed += 1;
                on_event(&RunEvent::Failed {
                    path,
                    error: e.to_string(),
                });
                continue;
            }
        };
        let Item::Request { file, .. } = r.item else {
            continue;
        };
        let request = &file.request;
        if !request.vars_post.is_empty()
            || request.script_pre.is_some()
            || request.script_post.is_some()
            || request.tests.is_some()
            || file.doc.block("assert").is_some()
        {
            tracing::info!(path = %path.display(), "scripts, tests, asserts and post-response vars run in Phase 3");
        }
        on_event(&RunEvent::Started {
            path: path.clone(),
            name: request.name.clone(),
            method: resolved.method.as_str().to_owned(),
            url: resolved.url.clone(),
        });
        match session.send(&resolved).await {
            Ok(res) => {
                report.passed += 1;
                on_event(&RunEvent::Passed {
                    path,
                    status: res.status,
                    elapsed: res.elapsed,
                    size: res.size,
                });
            }
            Err(e) => {
                report.failed += 1;
                on_event(&RunEvent::Failed {
                    path,
                    error: e.to_string(),
                });
            }
        }
    }
    report.elapsed = start.elapsed();
    Ok(report)
}

/// Why a request was not sent: skipped like Bruno does, or failed to resolve.
enum NotRunnable {
    Skip(String),
    Fail(ResolveError),
}

/// The resolve step shared by `run` and `send_one`.
fn prepare(ctx: &ResolveContext<'_>, r: &RequestRef<'_>) -> Result<ResolvedRequest, NotRunnable> {
    let file = match r.item {
        Item::Request { file, .. } => file,
        Item::Invalid { error, .. } => {
            return Err(NotRunnable::Skip(format!("parse error: {error}")));
        }
        Item::Folder(_) => {
            return Err(NotRunnable::Fail(ResolveError::NotARequest(
                r.path.to_path_buf(),
            )));
        }
    };
    if file.request.kind != "http" {
        return Err(NotRunnable::Skip(format!(
            "unsupported request type {}",
            file.request.kind
        )));
    }
    resolve(ctx, r).map_err(|e| match e {
        ResolveError::Unsupported(what) => NotRunnable::Skip(format!("unsupported {what}")),
        e => NotRunnable::Fail(e),
    })
}

pub struct SendInput<'a> {
    /// `/`-separated path of the request file relative to the collection root.
    pub item_id: &'a str,
    /// Unsaved edits to send instead of the file's request, with the file's scopes.
    pub draft: Option<&'a HttpRequest>,
    pub env: Option<&'a str>,
}

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("environment `{0}` not found")]
    UnknownEnvironment(String),
    #[error("request `{0}` not found")]
    NotFound(String),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("cannot send: {0}")]
    Unsupported(String),
    #[error(transparent)]
    Resolve(ResolveError),
    #[error(transparent)]
    Exec(#[from] ExecError),
}

/// Sends one request of an open collection with its session, runtime
/// variables and in-memory secrets (as environment overrides).
pub async fn send_one(
    open: &OpenCollection,
    input: SendInput<'_>,
) -> Result<ResponseSummary, SendError> {
    let collection = open.collection();
    let environment = match input.env {
        Some(name) => Some(
            collection
                .environment(name)
                .ok_or_else(|| SendError::UnknownEnvironment(name.to_owned()))?,
        ),
        None => None,
    };
    let path = store::resolve_item_path(&collection.root, input.item_id)?;
    let found = collection
        .requests_under(&path)
        .into_iter()
        .find(|r| r.path == path)
        .ok_or_else(|| SendError::NotFound(input.item_id.to_owned()))?;
    let draft_item;
    let target = match (input.draft, found.item) {
        (Some(draft), Item::Request { file, .. }) => {
            draft_item = Item::Request {
                path: path.clone(),
                file: Box::new(RequestFile {
                    doc: file.doc.clone(),
                    request: draft.clone(),
                }),
            };
            RequestRef {
                item: &draft_item,
                ..found
            }
        }
        _ => found,
    };
    let env_overrides = input.env.map(|e| open.secrets(e)).unwrap_or_default();
    let runtime = open.runtime();
    let ctx = ResolveContext {
        collection: &collection,
        environment,
        env_overrides: &env_overrides,
        runtime: &runtime,
    };
    let resolved = prepare(&ctx, &target).map_err(|e| match e {
        NotRunnable::Skip(reason) => SendError::Unsupported(reason),
        NotRunnable::Fail(e) => SendError::Resolve(e),
    })?;
    Ok(open.session.send(&resolved).await?)
}
