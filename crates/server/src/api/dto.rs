//! Wire types for the REST and WebSocket API. Field names are camelCase;
//! TypeScript definitions are generated from these with ts-rs.

use std::collections::BTreeMap;

use plover_core::request::{
    Auth, Body, HttpRequest, KeyValue, Method, MultipartField, MultipartValue, RequestSettings,
};
use plover_core::runner::{RequestOutcome, ScriptFailure};
use plover_core::script::{AssertResult, ConsoleLine, LogLevel, ScriptError, TestResult};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct KeyValueDto {
    pub name: String,
    pub value: String,
    pub enabled: bool,
    /// `@name` request-local variable (vars blocks only).
    #[serde(default)]
    pub local: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MultipartValueDto {
    Text { value: String },
    Files { paths: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MultipartFieldDto {
    pub name: String,
    pub value: MultipartValueDto,
    pub enabled: bool,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum BodyDto {
    None,
    Json {
        text: String,
    },
    Text {
        text: String,
    },
    Xml {
        text: String,
    },
    FormUrlEncoded {
        fields: Vec<KeyValueDto>,
    },
    MultipartForm {
        fields: Vec<MultipartFieldDto>,
    },
    /// A body mode this version keeps but does not send (graphql, sparql, file, ...).
    Unsupported {
        name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum AuthDto {
    None,
    Inherit,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    /// An auth mode this version keeps but does not apply (oauth2, awsv4, ...).
    Unsupported {
        name: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RequestSettingsDto {
    pub follow_redirects: Option<bool>,
    pub max_redirects: Option<u32>,
    #[ts(type = "number | null")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequestDto {
    pub name: String,
    pub seq: f64,
    /// `meta.type`, usually `http`.
    pub kind: String,
    /// Uppercase method name; any other string is a custom method.
    pub method: String,
    pub url: String,
    pub query: Vec<KeyValueDto>,
    pub path_params: Vec<KeyValueDto>,
    pub headers: Vec<KeyValueDto>,
    pub body: BodyDto,
    /// Bodies of the other modes present in the file (Bruno keeps one per mode).
    #[serde(default)]
    pub other_bodies: Vec<BodyDto>,
    pub auth: AuthDto,
    pub vars_pre: Vec<KeyValueDto>,
    pub vars_post: Vec<KeyValueDto>,
    pub script_pre: Option<String>,
    pub script_post: Option<String>,
    pub tests: Option<String>,
    /// The `assert` block: `name` is the left-hand expression, `value` is `operator rhs`.
    #[serde(default)]
    pub asserts: Vec<KeyValueDto>,
    pub settings: RequestSettingsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSummaryDto {
    pub id: String,
    pub name: String,
    pub root: String,
    /// `false` for a remembered collection that could not be reopened (moved, deleted).
    pub available: bool,
    /// Why an unavailable collection could not be opened.
    pub error: Option<String>,
}

/// A recently opened collection.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecentDto {
    /// Identifies the entry (`DELETE /api/recent?root=`).
    pub root: String,
    /// For display and for reopening (`POST /api/collections`).
    pub path: String,
    pub name: String,
    /// Unix milliseconds.
    #[ts(type = "number")]
    pub last_opened: u64,
    /// Currently open.
    pub open: bool,
}

/// Window layout shared by all collections (`GET/PUT /api/layout`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct LayoutDto {
    /// The dragged sidebar width in CSS pixels; `null` is the default width.
    pub sidebar_width: Option<u32>,
}

impl From<plover_core::app_state::LayoutState> for LayoutDto {
    fn from(l: plover_core::app_state::LayoutState) -> Self {
        LayoutDto {
            sidebar_width: l.sidebar_width,
        }
    }
}

impl From<LayoutDto> for plover_core::app_state::LayoutState {
    fn from(l: LayoutDto) -> Self {
        plover_core::app_state::LayoutState {
            sidebar_width: l.sidebar_width,
        }
    }
}

/// One collection's layout in the UI, remembered between starts. Unsaved drafts are not.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct UiStateDto {
    /// Item IDs of the open request tabs, in order.
    pub tabs: Vec<String>,
    pub active: Option<String>,
    /// Item IDs of expanded folders.
    pub expanded: Vec<String>,
    /// Selected environment.
    pub env: Option<String>,
    /// The whole collection is folded in the sidebar.
    pub collapsed: bool,
}

impl From<plover_core::app_state::UiState> for UiStateDto {
    fn from(u: plover_core::app_state::UiState) -> Self {
        UiStateDto {
            tabs: u.tabs,
            active: u.active,
            expanded: u.expanded,
            env: u.env,
            collapsed: u.collapsed,
        }
    }
}

impl From<UiStateDto> for plover_core::app_state::UiState {
    fn from(u: UiStateDto) -> Self {
        plover_core::app_state::UiState {
            tabs: u.tabs,
            active: u.active,
            expanded: u.expanded,
            env: u.env,
            collapsed: u.collapsed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TreeNodeKind {
    Folder,
    Request,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TreeNodeDto {
    pub kind: TreeNodeKind,
    /// `/`-separated path relative to the collection root; `""` for the root.
    pub id: String,
    pub name: String,
    pub method: Option<String>,
    pub seq: Option<f64>,
    /// Parse error of an invalid file.
    pub error: Option<String>,
    /// Present on folders only.
    pub children: Option<Vec<TreeNodeDto>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDto {
    pub name: String,
    pub vars: Vec<KeyValueDto>,
    pub secret_names: Vec<String>,
    /// Secrets that have a value and where each is kept (values are never sent).
    pub secrets_set: Vec<SecretStateDto>,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SecretStateDto {
    pub name: String,
    /// `"keychain"` or `"memory"` (kept only until Plover quits).
    #[ts(type = "\"keychain\" | \"memory\"")]
    pub backend: String,
}

impl SecretStateDto {
    pub fn backend_name(backend: plover_core::secrets::SecretBackend) -> String {
        match backend {
            plover_core::secrets::SecretBackend::Keychain => "keychain",
            plover_core::secrets::SecretBackend::Memory => "memory",
        }
        .to_owned()
    }
}

/// Result of setting secrets: one warning per value kept in memory instead of the keychain.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SecretsResultDto {
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResponseDto {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    /// Decoded body length in bytes. When `sizeComplete` is false, reading
    /// stopped at the server's cap and this is the number of bytes read.
    #[ts(type = "number")]
    pub size: usize,
    pub size_complete: bool,
    /// The body when it is valid UTF-8.
    pub body_text: Option<String>,
    /// The body otherwise.
    pub body_base64: Option<String>,
    /// The body was cut to the first 10 MiB.
    pub truncated: bool,
}

/// `POST /collections/{id}/send`: the response (absent when a pre-request script failed
/// or skipped the request) and what the request's scripts, asserts and tests reported.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SendResultDto {
    pub response: Option<ResponseDto>,
    pub tests: Vec<TestResultDto>,
    pub asserts: Vec<AssertResultDto>,
    pub console: Vec<ConsoleLineDto>,
    pub script_error: Option<ScriptErrorDto>,
    /// Requests the scripts sent with `bru.sendRequest` or `axios`.
    pub scripted_requests: Vec<ScriptedRequestDto>,
    /// Why the request was not sent (`bru.runner.skipRequest()`).
    pub skipped: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScriptedRequestDto {
    /// `sendRequest` or `axios`.
    pub source: String,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub error: Option<String>,
    #[ts(type = "number")]
    pub duration_ms: u64,
}

impl From<&plover_core::session_host::ScriptedRequest> for ScriptedRequestDto {
    fn from(r: &plover_core::session_host::ScriptedRequest) -> Self {
        ScriptedRequestDto {
            source: r.source.clone(),
            method: r.method.clone(),
            url: r.url.clone(),
            status: r.status,
            error: r.error.clone(),
            duration_ms: r.duration_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TestResultDto {
    pub name: String,
    pub passed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AssertResultDto {
    pub lhs: String,
    pub rhs: String,
    pub operator: String,
    pub passed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleLineDto {
    /// `log`, `info`, `warn`, `error` or `debug`.
    pub level: String,
    pub text: String,
}

/// A script that failed: where (`phase`, `scope`, `line`) and why.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScriptErrorDto {
    /// `pre-request`, `post-response` or `tests`.
    pub phase: String,
    /// `thrown`, `forbiddenModule`, `timeout`, `outOfMemory`, `unsettled` or `internal`.
    pub kind: String,
    /// Error name for thrown errors, e.g. `TypeError`.
    pub name: Option<String>,
    pub message: String,
    /// `collection`, `folder <name>` or `request`.
    pub scope: Option<String>,
    /// 1-based line within that scope's script.
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl From<&TestResult> for TestResultDto {
    fn from(t: &TestResult) -> Self {
        TestResultDto {
            name: t.name.clone(),
            passed: t.passed,
            error: t.error.clone(),
        }
    }
}

impl From<&AssertResult> for AssertResultDto {
    fn from(a: &AssertResult) -> Self {
        AssertResultDto {
            lhs: a.lhs.clone(),
            rhs: a.rhs.clone(),
            operator: a.operator.clone(),
            passed: a.passed,
            error: a.error.clone(),
        }
    }
}

impl From<&ConsoleLine> for ConsoleLineDto {
    fn from(c: &ConsoleLine) -> Self {
        let level = match c.level {
            LogLevel::Log => "log",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
            LogLevel::Debug => "debug",
        };
        ConsoleLineDto {
            level: level.to_owned(),
            text: c.text.clone(),
        }
    }
}

impl From<&ScriptFailure> for ScriptErrorDto {
    fn from(f: &ScriptFailure) -> Self {
        let (kind, name, scope, line, column) = match &f.error {
            ScriptError::Thrown {
                name,
                scope,
                line,
                column,
                ..
            } => ("thrown", Some(name.clone()), scope.clone(), *line, *column),
            ScriptError::ForbiddenModule { .. } => ("forbiddenModule", None, None, None, None),
            ScriptError::Timeout(_) => ("timeout", None, None, None, None),
            ScriptError::OutOfMemory => ("outOfMemory", None, None, None, None),
            ScriptError::Unsettled => ("unsettled", None, None, None, None),
            ScriptError::Internal(_) => ("internal", None, None, None, None),
            ScriptError::ResultUnavailable(_) => ("resultUnavailable", None, None, None, None),
        };
        let message = match &f.error {
            ScriptError::Thrown { message, .. } => message.clone(),
            other => other.to_string(),
        };
        ScriptErrorDto {
            phase: f.phase.as_str().to_owned(),
            kind: kind.to_owned(),
            name,
            message,
            scope,
            line,
            column,
        }
    }
}

/// Test, assert, console and script-error lists of one request, as DTOs.
pub fn outcome_lists(
    o: &RequestOutcome,
) -> (
    Vec<TestResultDto>,
    Vec<AssertResultDto>,
    Vec<ConsoleLineDto>,
    Option<ScriptErrorDto>,
) {
    (
        o.tests.iter().map(Into::into).collect(),
        o.asserts.iter().map(Into::into).collect(),
        o.console.iter().map(Into::into).collect(),
        o.script_error.as_ref().map(Into::into),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OpenCollectionBody {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StoredRequestDto {
    pub request: HttpRequestDto,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SaveRequestBody {
    pub request: HttpRequestDto,
    /// Hash of the file as last read; a mismatch returns 409.
    pub hash: String,
}

/// A saved request as parsed back from the written file, with its new hash.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SavedRequestDto {
    pub request: HttpRequestDto,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HashDto {
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SendBody {
    /// The request file; `null` sends `draft` as a scratch request (collection scope only).
    #[serde(default)]
    pub item: Option<String>,
    #[serde(default)]
    pub draft: Option<HttpRequestDto>,
    #[serde(default)]
    pub env: Option<String>,
    /// Names this send for `POST …/send/{sendId}/cancel` (`[A-Za-z0-9-]{1,64}`).
    #[serde(default)]
    pub send_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SaveEnvironmentBody {
    pub vars: Vec<KeyValueDto>,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SecretsBody {
    /// Merged into the current values: a string sets a secret, `null` clears it.
    pub values: BTreeMap<String, Option<String>>,
    /// Also add the names that get a value to the file's `vars:secret` block, so they are
    /// known after a restart ("Move to secret" on a pasted curl).
    #[serde(default)]
    #[ts(optional)]
    pub declare: Option<bool>,
}

/// The file content found on disk when a save conflicts.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConflictDto {
    pub text: String,
    pub hash: String,
}

/// Body of every non-2xx API response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorDto {
    pub error: String,
    pub message: String,
    /// Present on 409 `conflict`.
    pub current: Option<ConflictDto>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunReportDto {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub tests_passed: u32,
    pub tests_failed: u32,
    pub asserts_passed: u32,
    pub asserts_failed: u32,
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    /// Iterations reached (1 for a plain run).
    pub iterations: u32,
}

/// One saved run, as `GET /collections/{id}/runs` lists it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunMetaDto {
    pub record_id: String,
    /// `app` or `plv`.
    pub source: String,
    /// What ran, relative to the collection root; `.` for the whole collection.
    pub target: String,
    pub env: Option<String>,
    #[ts(type = "number")]
    pub started_at_ms: u64,
    /// `completed` or `cancelled`.
    pub outcome: String,
    pub report: RunReportDto,
}

/// A saved run replayed as the events a live run sends, so the UI builds its rows the
/// same way. Events carry the record ID as `runId`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunDto {
    pub meta: RunMetaDto,
    pub label: String,
    pub events: Vec<super::runs::RunEventDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PartDetailDto {
    pub name: String,
    /// Text parts only.
    pub value: Option<String>,
    /// File parts only: the file name, never its contents.
    pub file_name: Option<String>,
    pub content_type: Option<String>,
}

/// A stored request or response body.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BodyDetailDto {
    /// `none`, `text`, `base64`, `form`, `multipart` or `omitted`.
    pub kind: String,
    pub text: Option<String>,
    pub base64: Option<String>,
    pub fields: Vec<(String, String)>,
    pub parts: Vec<PartDetailDto>,
    /// The whole body's size in bytes.
    #[ts(type = "number")]
    pub size: u64,
    /// Only the first 1 MiB was kept.
    pub truncated: bool,
    /// Why the body was not kept (`kind == "omitted"`).
    pub omitted: Option<String>,
}

/// The request of a run result as it was sent.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RequestDetailDto {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: BodyDetailDto,
}

/// `GET /collections/{id}/runs/{runId}/results/{index}`: one result of a run, masked.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunResultDetailDto {
    pub index: u32,
    pub item: String,
    pub name: String,
    /// `pass`, `fail`, `error` or `skipped`.
    pub status: String,
    pub error: Option<String>,
    pub skip_reason: Option<String>,
    /// `None` when the request was never sent.
    pub request: Option<RequestDetailDto>,
    /// How the response body was kept (cut at 1 MiB, or omitted), when there was one.
    pub response_body: Option<BodyDetailDto>,
    pub result: SendResultDto,
}

impl From<&plover_core::run_record::BodyDetail> for BodyDetailDto {
    fn from(b: &plover_core::run_record::BodyDetail) -> Self {
        use plover_core::run_record::{BodyDetail, PartDetail};
        let mut dto = BodyDetailDto {
            kind: String::new(),
            text: None,
            base64: None,
            fields: Vec::new(),
            parts: Vec::new(),
            size: u64::try_from(b.size()).unwrap_or(u64::MAX),
            truncated: false,
            omitted: None,
        };
        dto.kind = match b {
            BodyDetail::None => "none",
            BodyDetail::Text {
                text, truncated, ..
            } => {
                dto.text = Some(text.clone());
                dto.truncated = *truncated;
                "text"
            }
            BodyDetail::Base64 {
                data, truncated, ..
            } => {
                dto.base64 = Some(data.clone());
                dto.truncated = *truncated;
                "base64"
            }
            BodyDetail::Form { fields } => {
                dto.fields = fields.clone();
                "form"
            }
            BodyDetail::Multipart { parts } => {
                dto.parts = parts
                    .iter()
                    .map(|p| match p {
                        PartDetail::Text {
                            name,
                            value,
                            content_type,
                        } => PartDetailDto {
                            name: name.clone(),
                            value: Some(value.clone()),
                            file_name: None,
                            content_type: content_type.clone(),
                        },
                        PartDetail::File {
                            name,
                            file_name,
                            content_type,
                        } => PartDetailDto {
                            name: name.clone(),
                            value: None,
                            file_name: Some(file_name.clone()),
                            content_type: content_type.clone(),
                        },
                    })
                    .collect();
                "multipart"
            }
            BodyDetail::Omitted { reason, .. } => {
                dto.omitted = Some(reason.clone());
                "omitted"
            }
        }
        .to_owned();
        dto
    }
}

impl From<&plover_core::run_record::RequestDetail> for RequestDetailDto {
    fn from(r: &plover_core::run_record::RequestDetail) -> Self {
        RequestDetailDto {
            method: r.method.clone(),
            url: r.url.clone(),
            headers: r.headers.clone(),
            body: (&r.body).into(),
        }
    }
}

impl From<&KeyValue> for KeyValueDto {
    fn from(kv: &KeyValue) -> Self {
        KeyValueDto {
            name: kv.name.clone(),
            value: kv.value.clone(),
            enabled: kv.enabled,
            local: kv.local,
        }
    }
}

impl From<KeyValueDto> for KeyValue {
    fn from(kv: KeyValueDto) -> Self {
        KeyValue {
            name: kv.name,
            value: kv.value,
            enabled: kv.enabled,
            local: kv.local,
        }
    }
}

fn kvs(list: &[KeyValue]) -> Vec<KeyValueDto> {
    list.iter().map(KeyValueDto::from).collect()
}

fn from_kvs(list: Vec<KeyValueDto>) -> Vec<KeyValue> {
    list.into_iter().map(KeyValue::from).collect()
}

impl From<&HttpRequest> for HttpRequestDto {
    fn from(r: &HttpRequest) -> Self {
        HttpRequestDto {
            name: r.name.clone(),
            seq: r.seq,
            kind: r.kind.clone(),
            method: r.method.as_str().to_owned(),
            url: r.url.clone(),
            query: kvs(&r.query),
            path_params: kvs(&r.path_params),
            headers: kvs(&r.headers),
            body: body_dto(&r.body),
            other_bodies: r.other_bodies.iter().map(body_dto).collect(),
            auth: match &r.auth {
                Auth::None => AuthDto::None,
                Auth::Inherit => AuthDto::Inherit,
                Auth::Basic { username, password } => AuthDto::Basic {
                    username: username.clone(),
                    password: password.clone(),
                },
                Auth::Bearer { token } => AuthDto::Bearer {
                    token: token.clone(),
                },
                Auth::Unsupported(name) => AuthDto::Unsupported { name: name.clone() },
            },
            vars_pre: kvs(&r.vars_pre),
            vars_post: kvs(&r.vars_post),
            script_pre: r.script_pre.clone(),
            script_post: r.script_post.clone(),
            tests: r.tests.clone(),
            asserts: kvs(&r.asserts),
            settings: RequestSettingsDto {
                follow_redirects: r.settings.follow_redirects,
                max_redirects: r.settings.max_redirects,
                timeout_ms: r.settings.timeout_ms,
            },
        }
    }
}

impl From<HttpRequestDto> for HttpRequest {
    fn from(d: HttpRequestDto) -> Self {
        let method = Method::from_block_name(&d.method.to_ascii_lowercase())
            .unwrap_or(Method::Custom(d.method));
        HttpRequest {
            name: d.name,
            seq: d.seq,
            kind: d.kind,
            method,
            url: d.url,
            query: from_kvs(d.query),
            path_params: from_kvs(d.path_params),
            headers: from_kvs(d.headers),
            body: body_from_dto(d.body),
            other_bodies: d.other_bodies.into_iter().map(body_from_dto).collect(),
            auth: match d.auth {
                AuthDto::None => Auth::None,
                AuthDto::Inherit => Auth::Inherit,
                AuthDto::Basic { username, password } => Auth::Basic { username, password },
                AuthDto::Bearer { token } => Auth::Bearer { token },
                AuthDto::Unsupported { name } => Auth::Unsupported(name),
            },
            vars_pre: from_kvs(d.vars_pre),
            vars_post: from_kvs(d.vars_post),
            script_pre: d.script_pre,
            script_post: d.script_post,
            tests: d.tests,
            asserts: from_kvs(d.asserts),
            settings: RequestSettings {
                follow_redirects: d.settings.follow_redirects,
                max_redirects: d.settings.max_redirects,
                timeout_ms: d.settings.timeout_ms,
            },
        }
    }
}

fn body_dto(body: &Body) -> BodyDto {
    match body {
        Body::None => BodyDto::None,
        Body::Json(text) => BodyDto::Json { text: text.clone() },
        Body::Text(text) => BodyDto::Text { text: text.clone() },
        Body::Xml(text) => BodyDto::Xml { text: text.clone() },
        Body::FormUrlEncoded(fields) => BodyDto::FormUrlEncoded {
            fields: kvs(fields),
        },
        Body::Multipart(fields) => BodyDto::MultipartForm {
            fields: fields
                .iter()
                .map(|f| MultipartFieldDto {
                    name: f.name.clone(),
                    value: match &f.value {
                        MultipartValue::Text(value) => MultipartValueDto::Text {
                            value: value.clone(),
                        },
                        MultipartValue::Files(paths) => MultipartValueDto::Files {
                            paths: paths.clone(),
                        },
                    },
                    enabled: f.enabled,
                    content_type: f.content_type.clone(),
                })
                .collect(),
        },
        Body::Unsupported(name) => BodyDto::Unsupported { name: name.clone() },
    }
}

fn body_from_dto(body: BodyDto) -> Body {
    match body {
        BodyDto::None => Body::None,
        BodyDto::Json { text } => Body::Json(text),
        BodyDto::Text { text } => Body::Text(text),
        BodyDto::Xml { text } => Body::Xml(text),
        BodyDto::FormUrlEncoded { fields } => Body::FormUrlEncoded(from_kvs(fields)),
        BodyDto::MultipartForm { fields } => Body::Multipart(
            fields
                .into_iter()
                .map(|f| MultipartField {
                    name: f.name,
                    value: match f.value {
                        MultipartValueDto::Text { value } => MultipartValue::Text(value),
                        MultipartValueDto::Files { paths } => MultipartValue::Files(paths),
                    },
                    enabled: f.enabled,
                    content_type: f.content_type,
                })
                .collect(),
        ),
        BodyDto::Unsupported { name } => Body::Unsupported(name),
    }
}

/// All DTO declarations as one TypeScript module.
/// What this server can do beyond the web API (`GET /api/capabilities`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitiesDto {
    /// Running inside the desktop app.
    pub desktop: bool,
    pub native_picker: bool,
    pub open_external: bool,
    /// Reports can be saved through a native save dialog (`POST /native/save-report`).
    pub native_save: bool,
    /// A request file can be shown in the file manager (`POST /native/reveal`).
    pub reveal: bool,
    /// Where secrets are kept: `"keychain"` or `"memory"`.
    #[ts(type = "\"keychain\" | \"memory\"")]
    pub secret_backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PickedFolderDto {
    /// `null` when the user cancelled.
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeBody {
    /// The single-use launch code from the URL fragment (`#code=…`).
    pub code: String,
}

/// `POST /api/client-ready`: the page's startup finished, with its error if it failed.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ClientReadyBody {
    #[serde(default)]
    #[ts(optional)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenDto {
    pub token: String,
}

// ---- import from cURL and HAR, scratch requests

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CurlParseBody {
    pub text: String,
}

/// An option of a pasted curl command that is not part of the filled-in request.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CurlNoticeDto {
    pub option: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CurlParsedDto {
    pub request: HttpRequestDto,
    pub notices: Vec<CurlNoticeDto>,
}

/// `POST /collections/{id}/resolved-url`: the request whose URL to fill in.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedUrlBody {
    /// The request file; `null` for a scratch request (then `draft` is required).
    #[serde(default)]
    pub item: Option<String>,
    #[serde(default)]
    pub draft: Option<HttpRequestDto>,
    #[serde(default)]
    pub env: Option<String>,
}

/// The URL Send would use, with secret values replaced by `***`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedUrlDto {
    pub url: String,
    /// Some secret value was replaced.
    pub masked: bool,
}

/// `POST /native/reveal`: show a request file in the file manager (desktop only).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RevealBody {
    pub collection: String,
    pub item: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CopyCurlBody {
    /// The request file; `null` for a scratch request (then `draft` is required).
    #[serde(default)]
    pub item: Option<String>,
    #[serde(default)]
    pub draft: Option<HttpRequestDto>,
    #[serde(default)]
    pub env: Option<String>,
    /// Fill in variables for `env` (no scripts run); otherwise copy as written.
    #[serde(default)]
    pub resolve: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CurlTextDto {
    pub text: String,
    /// A secret value of the environment appears in `text`.
    pub contains_secret: bool,
    /// What was left out because Plover cannot send it.
    pub omitted: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateRequestBody {
    /// The folder ID (`""` is the collection root).
    pub folder: String,
    /// The display name; the file name is made from it.
    pub name: String,
    pub request: HttpRequestDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreatedRequestDto {
    pub item: String,
    pub request: HttpRequestDto,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarPreviewBody {
    /// The HAR file's text.
    pub har: String,
}

/// One row of the HAR import dialog (identical requests share a row).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarRowDto {
    #[ts(type = "number")]
    pub index: usize,
    pub method: String,
    pub url: String,
    pub host: String,
    pub status: Option<u16>,
    pub mime: String,
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    pub has_secrets: bool,
    /// Why the row is hidden and unticked by default; `null` when it is shown.
    #[ts(
        type = "\"image\" | \"script\" | \"style\" | \"font\" | \"media\" | \"sourceMap\" | \"preflight\" | \"dataUrl\" | \"extension\" | \"webSocket\" | null"
    )]
    pub hidden: Option<String>,
    #[ts(type = "number")]
    pub duplicates: usize,
}

/// What to import: entry indexes, the folder to import into (`""` is the root) with an
/// optional new subfolder, and the environment for the secrets (`null`: `Imported`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarImportBody {
    pub har: String,
    #[ts(type = "number[]")]
    pub entries: Vec<usize>,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub new_folder: Option<String>,
    #[serde(default)]
    pub env: Option<String>,
}

/// The import a `HarImportBody` would do, without writing anything. Names only.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarPlanDto {
    pub env: String,
    pub secret_names: Vec<String>,
    pub request_names: Vec<String>,
    pub notices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarImportResultDto {
    /// Item IDs of the request files written.
    pub items: Vec<String>,
    /// Secret variables created (names only).
    pub secret_names: Vec<String>,
    pub env: String,
    pub notices: Vec<String>,
    /// The step that failed; what was written before it stays.
    pub failed_at: Option<String>,
}

impl From<&plover_core::curl::CurlNotice> for CurlNoticeDto {
    fn from(n: &plover_core::curl::CurlNotice) -> Self {
        CurlNoticeDto {
            option: n.option.clone(),
            reason: n.reason.clone(),
        }
    }
}

pub fn typescript_bindings() -> String {
    use ts_rs::Config;
    let cfg = Config::default();
    let decls = [
        KeyValueDto::decl(&cfg),
        MultipartValueDto::decl(&cfg),
        MultipartFieldDto::decl(&cfg),
        BodyDto::decl(&cfg),
        AuthDto::decl(&cfg),
        RequestSettingsDto::decl(&cfg),
        HttpRequestDto::decl(&cfg),
        CollectionSummaryDto::decl(&cfg),
        RecentDto::decl(&cfg),
        UiStateDto::decl(&cfg),
        LayoutDto::decl(&cfg),
        TreeNodeKind::decl(&cfg),
        TreeNodeDto::decl(&cfg),
        EnvironmentDto::decl(&cfg),
        ResponseDto::decl(&cfg),
        TestResultDto::decl(&cfg),
        AssertResultDto::decl(&cfg),
        ConsoleLineDto::decl(&cfg),
        ScriptedRequestDto::decl(&cfg),
        ScriptErrorDto::decl(&cfg),
        SendResultDto::decl(&cfg),
        OpenCollectionBody::decl(&cfg),
        StoredRequestDto::decl(&cfg),
        SaveRequestBody::decl(&cfg),
        SavedRequestDto::decl(&cfg),
        HashDto::decl(&cfg),
        SendBody::decl(&cfg),
        SaveEnvironmentBody::decl(&cfg),
        SecretsBody::decl(&cfg),
        SecretStateDto::decl(&cfg),
        SecretsResultDto::decl(&cfg),
        ConflictDto::decl(&cfg),
        ApiErrorDto::decl(&cfg),
        RunReportDto::decl(&cfg),
        RunMetaDto::decl(&cfg),
        RunDto::decl(&cfg),
        PartDetailDto::decl(&cfg),
        BodyDetailDto::decl(&cfg),
        RequestDetailDto::decl(&cfg),
        RunResultDetailDto::decl(&cfg),
        super::runs::RunEventDto::decl(&cfg),
        super::runs::ClientMessage::decl(&cfg),
        CapabilitiesDto::decl(&cfg),
        PickedFolderDto::decl(&cfg),
        ExchangeBody::decl(&cfg),
        ClientReadyBody::decl(&cfg),
        SessionTokenDto::decl(&cfg),
        crate::events::ServerEvent::decl(&cfg),
        CurlParseBody::decl(&cfg),
        CurlNoticeDto::decl(&cfg),
        CurlParsedDto::decl(&cfg),
        CopyCurlBody::decl(&cfg),
        ResolvedUrlBody::decl(&cfg),
        super::vars::RuntimeVarBody::decl(&cfg),
        super::vars::VarsBody::decl(&cfg),
        super::vars::VarDto::decl(&cfg),
        super::vars::SetLaterDto::decl(&cfg),
        super::vars::VarsDto::decl(&cfg),
        super::vars::RuntimeVarsDto::decl(&cfg),
        ResolvedUrlDto::decl(&cfg),
        RevealBody::decl(&cfg),
        CurlTextDto::decl(&cfg),
        CreateRequestBody::decl(&cfg),
        CreatedRequestDto::decl(&cfg),
        HarPreviewBody::decl(&cfg),
        HarRowDto::decl(&cfg),
        HarImportBody::decl(&cfg),
        HarPlanDto::decl(&cfg),
        HarImportResultDto::decl(&cfg),
        DataFormatDto::decl(&cfg),
        DataFileDto::decl(&cfg),
        DataPreviewDto::decl(&cfg),
        SaveReportBody::decl(&cfg),
        SavedFileDto::decl(&cfg),
    ];
    let mut out = String::from(
        "// Generated from crates/server/src/api by ts-rs. Do not edit.\n\
         // Regenerate: PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings\n",
    );
    for decl in decls {
        out.push('\n');
        out.push_str("export ");
        out.push_str(&decl);
        out.push('\n');
    }
    out
}

/// A data file the browser read, for a data-driven run or its preview.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DataFileDto {
    pub format: DataFormatDto,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum DataFormatDto {
    Csv,
    Json,
}

impl From<DataFormatDto> for plover_core::data_file::DataFormat {
    fn from(f: DataFormatDto) -> Self {
        match f {
            DataFormatDto::Csv => plover_core::data_file::DataFormat::Csv,
            DataFormatDto::Json => plover_core::data_file::DataFormat::Json,
        }
    }
}

/// `POST /collections/{id}/data-file/preview`: counts and names only, never values.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DataPreviewDto {
    pub rows: u32,
    pub columns: Vec<String>,
}

/// `GET /collections/{id}/runs/{runId}/report?format=…`.
#[derive(Debug, Clone, Deserialize)]
pub struct ReportQuery {
    pub format: String,
}

/// `POST /native/save-report`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SaveReportBody {
    pub collection_id: String,
    pub run_id: String,
    #[ts(type = "\"html\" | \"json\" | \"junit\"")]
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SavedFileDto {
    /// `null` when the user cancelled the dialog.
    pub path: Option<String>,
}
