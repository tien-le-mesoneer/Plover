//! Run results collected from events, for the JUnit and JSON reporters.

pub mod html;
pub mod json;
pub mod junit;

use std::path::{Path, PathBuf};
use std::time::Duration;

use plover_core::runner::{RequestOutcome, RunEvent, ScriptPhase};
use plover_core::script::ScriptError;

/// One request of the run, as the reporters need it.
#[derive(Debug, Clone)]
pub struct RequestRecord {
    /// Relative to the collection root, `/`-separated.
    pub file: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub status: RecordStatus,
    pub outcome: RequestOutcome,
    pub error: Option<String>,
    pub skip_reason: Option<String>,
}

/// Bruno's per-result status (`pass`, `fail`, `error`, `skipped`): `fail` is a completed
/// request with failed tests or asserts, `error` one that got no response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStatus {
    Pass,
    Fail,
    Error,
    Skipped,
}

impl RecordStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RecordStatus::Pass => "pass",
            RecordStatus::Fail => "fail",
            RecordStatus::Error => "error",
            RecordStatus::Skipped => "skipped",
        }
    }
}

impl RequestRecord {
    /// The whole request, scripts included (Bruno's `runDuration`); the response time for
    /// records that were never timed (skipped).
    pub fn duration(&self) -> Duration {
        if self.outcome.duration.is_zero() {
            self.outcome.elapsed
        } else {
            self.outcome.duration
        }
    }

    /// Bruno records a post-response script error as a failed post-response test named
    /// "Post-Response Script Error" whose error is the thrown message
    /// (run-single-request.js); both reporters list it that way.
    pub fn post_response_script_error(&self) -> Option<String> {
        let failure = self.outcome.script_error.as_ref()?;
        (failure.phase == ScriptPhase::PostResponse).then(|| match &failure.error {
            ScriptError::Thrown { message, .. } => message.clone(),
            other => other.to_string(),
        })
    }
}

pub const POST_RESPONSE_SCRIPT_ERROR: &str = "Post-Response Script Error";

/// The error text the reporters show, like Bruno's: a script that stopped the request
/// (pre-request) reports the thrown message alone (`Request failed with status code
/// 401`), without the CLI's "pre-request script error (…): Error:" context.
fn report_error(error: &str, outcome: &RequestOutcome) -> String {
    match &outcome.script_error {
        Some(failure) if outcome.status.is_none() => match &failure.error {
            ScriptError::Thrown { message, .. } => message.clone(),
            other => other.to_string(),
        },
        _ => error.to_owned(),
    }
}

#[derive(Default)]
pub struct Collector {
    root: PathBuf,
    pub records: Vec<RequestRecord>,
    started: Option<(String, String, String)>,
}

impl Collector {
    pub fn new(root: &Path) -> Collector {
        Collector {
            root: root.to_path_buf(),
            ..Collector::default()
        }
    }

    pub fn event(&mut self, event: &RunEvent) {
        let record = |c: &mut Collector,
                      path: &Path,
                      status,
                      outcome: &RequestOutcome,
                      error: Option<&str>,
                      skip: Option<&str>| {
            let file = path
                .strip_prefix(&c.root)
                .unwrap_or(path)
                .components()
                .map(|p| p.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let (method, name, url) = c.started.take().unwrap_or_else(|| {
                let stem = Path::new(&file)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (String::new(), stem, String::new())
            });
            c.records.push(RequestRecord {
                file,
                name,
                method,
                url,
                status,
                outcome: outcome.clone(),
                error: error.map(str::to_owned),
                skip_reason: skip.map(str::to_owned),
            });
        };
        match event {
            RunEvent::Started {
                name, method, url, ..
            } => self.started = Some((method.clone(), name.clone(), url.clone())),
            RunEvent::Passed { path, outcome } => {
                record(self, path, RecordStatus::Pass, outcome, None, None)
            }
            RunEvent::Failed {
                path,
                name,
                error,
                outcome,
            } => {
                let status = if outcome.status.is_some() {
                    RecordStatus::Fail
                } else {
                    RecordStatus::Error
                };
                // A pre-request failure never sent the request, so no `Started` named it.
                if self.started.is_none() {
                    self.started = Some((String::new(), name.clone(), String::new()));
                }
                let error = report_error(error, outcome);
                record(self, path, status, outcome, Some(&error), None)
            }
            RunEvent::Skipped { path, name, reason } => {
                // Never started, so no `Started` event named it.
                self.started = Some((String::new(), name.clone(), String::new()));
                record(
                    self,
                    path,
                    RecordStatus::Skipped,
                    &RequestOutcome::default(),
                    None,
                    Some(reason),
                )
            }
            RunEvent::Finished { .. } => {}
        }
    }
}
