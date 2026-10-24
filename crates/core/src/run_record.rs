//! What a run keeps: one record per request with the request as sent and the response as
//! received (Bruno CLI keeps both in each result, run-single-request.js), built from run
//! events by a `RunRecorder`. History and every report use these records.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::executor::ResponseSummary;
use crate::resolve::{ResolvedBody, ResolvedPart, ResolvedRequest};
use crate::runner::{RequestOutcome, RunEvent, RunReport, ScriptPhase};
use crate::script::ScriptError;

/// Each stored request or response body keeps at most this many bytes.
pub const MAX_DETAIL_BODY: usize = 1 << 20;

/// The request as it was sent, after variables and scripts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDetail {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: BodyDetail,
}

/// The response as the run saw it, with a body replaced by `res.setBody`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseDetail {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: BodyDetail,
    pub url: String,
    pub size: usize,
    /// `false` when the executor stopped reading the body, so `size` is a lower bound.
    pub size_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BodyDetail {
    None,
    /// UTF-8 text; `size` is the whole body's length in bytes.
    Text {
        text: String,
        size: usize,
        truncated: bool,
    },
    /// Bytes that are not UTF-8, base64-encoded.
    Base64 {
        data: String,
        size: usize,
        truncated: bool,
    },
    Form {
        fields: Vec<(String, String)>,
    },
    Multipart {
        parts: Vec<PartDetail>,
    },
    /// Not kept, e.g. after the run's body budget ran out.
    Omitted {
        reason: String,
        size: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PartDetail {
    #[serde(rename_all = "camelCase")]
    Text {
        name: String,
        value: String,
        content_type: Option<String>,
    },
    /// Only the file name is kept, never the file's bytes.
    #[serde(rename_all = "camelCase")]
    File {
        name: String,
        file_name: String,
        content_type: Option<String>,
    },
}

impl RequestDetail {
    pub fn of(resolved: &ResolvedRequest) -> RequestDetail {
        let body = match &resolved.body {
            ResolvedBody::None => BodyDetail::None,
            ResolvedBody::Bytes(bytes) => BodyDetail::of_bytes(bytes, bytes.len(), false),
            ResolvedBody::Form(fields) => BodyDetail::Form {
                fields: fields.clone(),
            },
            ResolvedBody::Multipart(parts) => BodyDetail::Multipart {
                parts: parts
                    .iter()
                    .map(|p| match p {
                        ResolvedPart::Text {
                            name,
                            value,
                            content_type,
                        } => PartDetail::Text {
                            name: name.clone(),
                            value: value.clone(),
                            content_type: content_type.clone(),
                        },
                        ResolvedPart::File {
                            name,
                            path,
                            content_type,
                        } => PartDetail::File {
                            name: name.clone(),
                            file_name: path
                                .file_name()
                                .map(|f| f.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            content_type: content_type.clone(),
                        },
                    })
                    .collect(),
            },
        };
        RequestDetail {
            method: resolved.method.as_str().to_owned(),
            url: resolved.url.clone(),
            headers: resolved.headers.clone(),
            body,
        }
    }
}

impl ResponseDetail {
    pub fn of(response: &ResponseSummary, url: &str) -> ResponseDetail {
        ResponseDetail {
            status: response.status,
            status_text: status_text(response.status),
            headers: response.headers.clone(),
            body: BodyDetail::of_bytes(&response.body, response.body.len(), response.truncated),
            url: url.to_owned(),
            size: response.size,
            size_complete: !response.truncated,
        }
    }
}

/// The standard reason phrase, as scripts see it in `res.statusText`.
pub fn status_text(status: u16) -> String {
    reqwest::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or_default()
        .to_owned()
}

impl BodyDetail {
    /// `size` is the whole body's length; `truncated` says `bytes` is already cut short.
    pub fn of_bytes(bytes: &[u8], size: usize, truncated: bool) -> BodyDetail {
        if bytes.is_empty() && !truncated {
            return BodyDetail::None;
        }
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                let mut end = text.len().min(MAX_DETAIL_BODY);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                BodyDetail::Text {
                    text: text[..end].to_owned(),
                    size,
                    truncated: truncated || end < text.len(),
                }
            }
            // A body cut mid-character by the executor is still text.
            Err(e) if truncated && e.error_len().is_none() => {
                BodyDetail::of_bytes(&bytes[..e.valid_up_to()], size, true)
            }
            Err(_) => {
                let end = bytes.len().min(MAX_DETAIL_BODY);
                BodyDetail::Base64 {
                    data: base64::engine::general_purpose::STANDARD.encode(&bytes[..end]),
                    size,
                    truncated: truncated || end < bytes.len(),
                }
            }
        }
    }

    /// Bytes this body takes in a stored run, for the run's body budget.
    pub fn stored_len(&self) -> usize {
        match self {
            BodyDetail::None | BodyDetail::Omitted { .. } => 0,
            BodyDetail::Text { text, .. } => text.len(),
            BodyDetail::Base64 { data, .. } => data.len(),
            BodyDetail::Form { fields } => fields.iter().map(|(k, v)| k.len() + v.len()).sum(),
            BodyDetail::Multipart { parts } => parts
                .iter()
                .map(|p| match p {
                    PartDetail::Text { name, value, .. } => name.len() + value.len(),
                    PartDetail::File {
                        name, file_name, ..
                    } => name.len() + file_name.len(),
                })
                .sum(),
        }
    }

    /// The body's full size in bytes, as far as it is known.
    pub fn size(&self) -> usize {
        match self {
            BodyDetail::Text { size, .. }
            | BodyDetail::Base64 { size, .. }
            | BodyDetail::Omitted { size, .. } => *size,
            other => other.stored_len(),
        }
    }
}

/// Version of the stored run format; files with another version are skipped.
pub const RUN_RECORD_VERSION: u32 = 1;
/// Request and response bodies one run keeps in total; later bodies are omitted.
pub const MAX_RUN_BODIES: usize = 50 << 20;
const RUN_SIZE_LIMIT: &str = "run size limit";

/// `<UTC yyyymmddThhmmssZ>-<6 lowercase base32 chars>`, so IDs sort by start time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(pub String);

impl RunId {
    pub fn new(now: SystemTime) -> RunId {
        const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
        let mut random = [0u8; 4];
        if getrandom::fill(&mut random).is_err() {
            // Uniqueness then rests on the nanoseconds.
            let nanos = now
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos();
            random = nanos.to_le_bytes();
        }
        let bits = u32::from_le_bytes(random);
        let suffix: String = (0..6)
            .map(|i| ALPHABET[((bits >> (i * 5)) & 31) as usize] as char)
            .collect();
        RunId(format!("{}-{suffix}", crate::time::compact_utc(now)))
    }

    /// `None` unless `s` has exactly the ID format, so an ID never escapes a folder.
    pub fn parse(s: &str) -> Option<RunId> {
        let b = s.as_bytes();
        let ok = b.len() == 23
            && b[..8].iter().all(u8::is_ascii_digit)
            && b[8] == b'T'
            && b[9..15].iter().all(u8::is_ascii_digit)
            && b[15] == b'Z'
            && b[16] == b'-'
            && b[17..]
                .iter()
                .all(|c| c.is_ascii_lowercase() || (b'2'..=b'7').contains(c));
        ok.then(|| RunId(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunSource {
    App,
    Plv,
}

impl RunSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunSource::App => "app",
            RunSource::Plv => "plv",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunOutcome {
    Completed,
    Cancelled,
}

impl RunOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunOutcome::Completed => "completed",
            RunOutcome::Cancelled => "cancelled",
        }
    }
}

/// Bruno's per-result status (`pass`, `fail`, `error`, `skipped`): `fail` is a completed
/// request with failed tests or asserts, `error` one that got no response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultStatus {
    Pass,
    Fail,
    Error,
    Skipped,
}

impl ResultStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResultStatus::Pass => "pass",
            ResultStatus::Fail => "fail",
            ResultStatus::Error => "error",
            ResultStatus::Skipped => "skipped",
        }
    }
}

/// Which run this is. A run file is self-contained: it names its collection, so it can be
/// shown after the collection moved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunHeader {
    pub id: RunId,
    pub source: RunSource,
    /// The canonical collection root.
    pub collection_root: String,
    pub collection_name: String,
    /// What ran, relative to the root and `/`-separated; `.` for the whole collection.
    pub target: String,
    pub env: Option<String>,
    pub started_at_ms: u64,
}

/// One request of a run, as history and the reporters need it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultRecord {
    /// Position in the run; a request run twice (`setNextRequest`) has two indices.
    pub index: usize,
    /// Relative to the collection root, `/`-separated.
    pub file: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub status: ResultStatus,
    pub outcome: RequestOutcome,
    pub error: Option<String>,
    pub skip_reason: Option<String>,
}

pub const POST_RESPONSE_SCRIPT_ERROR: &str = "Post-Response Script Error";

impl ResultRecord {
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
    /// (run-single-request.js); the reporters list it that way.
    pub fn post_response_script_error(&self) -> Option<String> {
        let failure = self.outcome.script_error.as_ref()?;
        (failure.phase == ScriptPhase::PostResponse).then(|| match &failure.error {
            ScriptError::Thrown { message, .. } => message.clone(),
            other => other.to_string(),
        })
    }
}

/// A whole run, as stored in history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub version: u32,
    #[serde(flatten)]
    pub header: RunHeader,
    pub outcome: RunOutcome,
    pub report: RunReport,
    pub results: Vec<ResultRecord>,
}

/// Turns the events of one run into `ResultRecord`s.
pub struct RunRecorder {
    header: RunHeader,
    root: PathBuf,
    results: Vec<ResultRecord>,
    /// From the `Started` event of the request in flight: method, name, URL.
    started: Option<(String, String, String)>,
    #[allow(dead_code)] // read by `finish` (masking)
    created: Instant,
    bodies_used: usize,
    bodies_exhausted: bool,
    #[allow(dead_code)] // read by `finish` (masking)
    start_secrets: Vec<String>,
}

impl RunRecorder {
    pub fn new(root: &Path, header: RunHeader, start_secrets: Vec<String>) -> RunRecorder {
        RunRecorder {
            header,
            root: root.to_path_buf(),
            results: Vec::new(),
            started: None,
            created: Instant::now(),
            bodies_used: 0,
            bodies_exhausted: false,
            start_secrets,
        }
    }

    pub fn header(&self) -> &RunHeader {
        &self.header
    }

    /// Unmasked, for live terminal output.
    pub fn results(&self) -> &[ResultRecord] {
        &self.results
    }

    /// The index of the result this event completed, if any.
    pub fn event(&mut self, event: &RunEvent) -> Option<usize> {
        match event {
            RunEvent::Started {
                name, method, url, ..
            } => {
                self.started = Some((method.clone(), name.clone(), url.clone()));
                None
            }
            RunEvent::Passed { path, outcome } => {
                Some(self.record(path, ResultStatus::Pass, outcome, None, None))
            }
            RunEvent::Failed {
                path,
                name,
                error,
                outcome,
            } => {
                let status = if outcome.status.is_some() {
                    ResultStatus::Fail
                } else {
                    ResultStatus::Error
                };
                // A pre-request failure never sent the request, so no `Started` named it.
                if self.started.is_none() {
                    self.started = Some((String::new(), name.clone(), String::new()));
                }
                let error = report_error(error, outcome);
                Some(self.record(path, status, outcome, Some(error), None))
            }
            RunEvent::Skipped { path, name, reason } => {
                // Never started, so no `Started` event named it.
                self.started = Some((String::new(), name.clone(), String::new()));
                Some(self.record(
                    path,
                    ResultStatus::Skipped,
                    &RequestOutcome::default(),
                    None,
                    Some(reason.clone()),
                ))
            }
            RunEvent::Finished { .. } => None,
        }
    }

    fn record(
        &mut self,
        path: &Path,
        status: ResultStatus,
        outcome: &RequestOutcome,
        error: Option<String>,
        skip_reason: Option<String>,
    ) -> usize {
        let file = path
            .strip_prefix(&self.root)
            .unwrap_or(path)
            .components()
            .map(|p| p.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let (method, name, url) = self.started.take().unwrap_or_else(|| {
            let stem = Path::new(&file)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            (String::new(), stem, String::new())
        });
        let mut outcome = outcome.clone();
        if let Some(request) = outcome.request.as_mut() {
            self.budget(&mut request.body);
        }
        if let Some(response) = outcome.response.as_mut() {
            self.budget(&mut response.body);
        }
        let index = self.results.len();
        self.results.push(ResultRecord {
            index,
            file,
            name,
            method,
            url,
            status,
            outcome,
            error,
            skip_reason,
        });
        index
    }

    /// Keeps `body` while the run's body budget lasts, in arrival order.
    fn budget(&mut self, body: &mut BodyDetail) {
        let len = body.stored_len();
        if len == 0 {
            return;
        }
        if !self.bodies_exhausted && self.bodies_used + len <= MAX_RUN_BODIES {
            self.bodies_used += len;
            return;
        }
        self.bodies_exhausted = true;
        *body = BodyDetail::Omitted {
            reason: RUN_SIZE_LIMIT.to_owned(),
            size: body.size(),
        };
    }
}

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
