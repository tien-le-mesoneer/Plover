//! Runs the requests under a target in Bruno's order and reports each one as a `RunEvent`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::collection::{Collection, Item};
use crate::executor::{ExecError, ExecOptions, Session};
use crate::resolve::{ResolveContext, ResolveError, resolve};
use crate::vars::VarMap;

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
        let file = match r.item {
            Item::Request { file, .. } => file,
            Item::Invalid { error, .. } => {
                report.skipped += 1;
                on_event(&RunEvent::Skipped {
                    path,
                    reason: format!("parse error: {error}"),
                });
                continue;
            }
            Item::Folder(_) => continue,
        };
        let request = &file.request;
        if request.kind != "http" {
            report.skipped += 1;
            on_event(&RunEvent::Skipped {
                path,
                reason: format!("unsupported request type {}", request.kind),
            });
            continue;
        }
        let resolved = match resolve(&ctx, &r) {
            Ok(resolved) => resolved,
            Err(ResolveError::Unsupported(what)) => {
                report.skipped += 1;
                on_event(&RunEvent::Skipped {
                    path,
                    reason: format!("unsupported {what}"),
                });
                continue;
            }
            Err(e) => {
                report.failed += 1;
                on_event(&RunEvent::Failed {
                    path,
                    error: e.to_string(),
                });
                continue;
            }
        };
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
