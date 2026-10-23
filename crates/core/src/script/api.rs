//! What scripts can see and change, exchanged with `api.js` as JSON. The JS side mirrors
//! Bruno's `bru`, `req`, `res` and `test` objects; this side is plain data the runner owns.

use serde::{Deserialize, Serialize};

use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use super::engine::{ConsoleLine, EngineJob, Main, ScriptEngine, ScriptError, Segment};
use crate::vars::{InterpolateError, InterpolateLimits, ValueMap, VarStack};

/// Variable scopes a script reads and writes. Folder variables are merged outermost
/// first, so inner folders win.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptVars {
    pub runtime: ValueMap,
    pub environment: ValueMap,
    pub collection: ValueMap,
    pub folder: ValueMap,
    pub request: ValueMap,
}

impl ScriptVars {
    /// The interpolation stack these variables form (Phase 1 precedence).
    pub fn var_stack(&self) -> VarStack {
        VarStack::from_values(
            self.runtime.clone(),
            self.request.clone(),
            vec![self.folder.clone()],
            self.environment.clone(),
            self.collection.clone(),
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptInfo {
    pub env_name: Option<String>,
    pub collection_name: String,
    /// `bru.cwd()`.
    pub collection_path: String,
    pub request_name: String,
    /// `bru.getProcessEnv` and `{{process.env.NAME}}`: the process environment and `.env`.
    pub process_env: ValueMap,
}

/// The request as scripts see it. Before the request is sent this is the prepared,
/// not yet interpolated request (Bruno runs pre-request scripts before `interpolateVars`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptRequest {
    pub url: String,
    pub method: String,
    /// In order; names are kept exactly as written.
    pub headers: Vec<(String, String)>,
    pub body: ScriptBody,
    pub timeout_ms: Option<u64>,
    pub max_redirects: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum ScriptBody {
    #[default]
    None,
    /// JSON, text and XML bodies.
    Text { text: String },
    /// `body:form-urlencoded`, as name/value pairs.
    Form { fields: Vec<(String, String)> },
    /// Multipart bodies are sent as written; scripts can't read or replace them yet.
    Multipart,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    /// Decoded body text; `res.body` is its JSON value when it parses (Bruno CLI
    /// `parseDataFromResponse`), else this string.
    pub body: String,
    pub response_time_ms: u64,
    pub url: String,
    pub size: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptContext {
    pub vars: ScriptVars,
    pub info: ScriptInfo,
    pub req: ScriptRequest,
    pub res: Option<ScriptResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextRequest {
    /// `setNextRequest(name)`: continue the run at the request with this name.
    Name(String),
    /// `setNextRequest(null)`: stop the run after this request.
    Stop,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flow {
    pub skip_request: bool,
    pub stop_execution: bool,
    pub next_request: Option<NextRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssertResult {
    pub lhs: String,
    pub rhs: String,
    pub operator: String,
    pub passed: bool,
    pub error: Option<String>,
}

/// Everything one script phase produced. On error, the state up to the failure is kept
/// (Bruno's `partialResults`).
#[derive(Debug, Clone, Default)]
pub struct PhaseOutput {
    pub vars: ScriptVars,
    pub req: ScriptRequest,
    /// `res.setBody(value)`; later phases and the reported response see it.
    pub res_body: Option<serde_json::Value>,
    pub tests: Vec<TestResult>,
    pub asserts: Vec<AssertResult>,
    pub flow: Flow,
    pub console: Vec<ConsoleLine>,
    pub error: Option<ScriptError>,
}

/// What `api.js` reports back (`__plv.resultParts`).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Reported {
    vars: Option<ScriptVars>,
    req: Option<ScriptRequest>,
    res_body_set: bool,
    res_body: serde_json::Value,
    tests: Vec<TestResult>,
    asserts: Vec<AssertResult>,
    skip_request: bool,
    stop_execution: bool,
    next_request_set: bool,
    next_request: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Input<'a> {
    vars: &'a ScriptVars,
    info: &'a ScriptInfo,
    req: &'a ScriptRequest,
    res: Option<&'a ScriptResponse>,
    asserts: &'a [(String, String)],
    post_vars: &'a [(String, String)],
}

pub type HostFuture<'a> =
    Pin<Box<dyn Future<Output = Result<serde_json::Value, String>> + Send + 'a>>;

/// Answers what scripts ask of the session (HTTP, cookies). Runs in the caller's async
/// context while the script waits, bounded by the script phase's deadline.
pub trait ScriptHost: Sync {
    fn call<'a>(&'a self, op: &'a str, args: serde_json::Value) -> HostFuture<'a>;
}

/// Runs script segments (collection, folders, request — in the order given).
pub async fn run_script(
    engine: &ScriptEngine,
    ctx: &ScriptContext,
    segments: Vec<Segment>,
) -> PhaseOutput {
    run(engine, ctx, Main::Segments(segments), &[], &[], None).await
}

/// Like `run_script`, with `bru.sendRequest`, `axios` and `bru.cookies` served by `host`.
pub async fn run_script_with_host(
    engine: &ScriptEngine,
    ctx: &ScriptContext,
    segments: Vec<Segment>,
    host: &dyn ScriptHost,
) -> PhaseOutput {
    run(engine, ctx, Main::Segments(segments), &[], &[], Some(host)).await
}

/// Evaluates `assert` entries (enabled ones, as `(lhs, "operator rhs")`) like Bruno's
/// `AssertRuntime`.
pub async fn run_asserts(
    engine: &ScriptEngine,
    ctx: &ScriptContext,
    asserts: &[(String, String)],
) -> PhaseOutput {
    run(
        engine,
        ctx,
        Main::Call("__plv.runAsserts()".into()),
        asserts,
        &[],
        None,
    )
    .await
}

/// Evaluates `vars:post-response` entries (enabled ones) as JS expressions and stores the
/// results as runtime variables, like Bruno's `VarsRuntime.runPostResponseVars`.
pub async fn run_post_response_vars(
    engine: &ScriptEngine,
    ctx: &ScriptContext,
    vars: &[(String, String)],
) -> PhaseOutput {
    run(
        engine,
        ctx,
        Main::Call("__plv.runPostVars()".into()),
        &[],
        vars,
        None,
    )
    .await
}

async fn run(
    engine: &ScriptEngine,
    ctx: &ScriptContext,
    main: Main,
    asserts: &[(String, String)],
    post_vars: &[(String, String)],
    host: Option<&dyn ScriptHost>,
) -> PhaseOutput {
    let input = Input {
        vars: &ctx.vars,
        info: &ctx.info,
        req: &ctx.req,
        res: ctx.res.as_ref(),
        asserts,
        post_vars,
    };
    let input = match serde_json::to_value(&input) {
        Ok(v) => v,
        Err(e) => {
            return PhaseOutput {
                vars: ctx.vars.clone(),
                req: ctx.req.clone(),
                error: Some(ScriptError::Internal(e.to_string())),
                ..PhaseOutput::default()
            };
        }
    };
    let (tx, mut requests) = tokio::sync::mpsc::unbounded_channel();
    let job = EngineJob {
        input,
        main,
        host: host.map(|_| tx),
    };
    let running = engine.run(job);
    tokio::pin!(running);
    let out = loop {
        tokio::select! {
            out = &mut running => break out,
            Some(request) = requests.recv(), if host.is_some() => {
                let Some(host) = host else { continue };
                let left = request.deadline.saturating_duration_since(Instant::now());
                let answer = tokio::time::timeout(left, host.call(&request.op, request.args))
                    .await
                    .unwrap_or_else(|_| Err("interrupted".to_owned()));
                let _ = request.reply.send(answer);
            }
        }
    };
    let reported: Reported =
        serde_json::from_value(serde_json::Value::Object(out.output)).unwrap_or_default();
    PhaseOutput {
        vars: reported.vars.unwrap_or_else(|| ctx.vars.clone()),
        req: reported.req.unwrap_or_else(|| ctx.req.clone()),
        res_body: reported.res_body_set.then_some(reported.res_body),
        tests: reported.tests,
        asserts: reported.asserts,
        flow: Flow {
            skip_request: reported.skip_request,
            stop_execution: reported.stop_execution,
            next_request: reported
                .next_request_set
                .then_some(match reported.next_request {
                    Some(name) => NextRequest::Name(name),
                    None => NextRequest::Stop,
                }),
        },
        console: out.console,
        error: out.error,
    }
}

/// Variable layers as `api.js` sends them to `__plvInterpolate`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Layers {
    runtime: ValueMap,
    request: ValueMap,
    folder: ValueMap,
    environment: ValueMap,
    collection: ValueMap,
    process_env: ValueMap,
}

/// `bru.interpolate` and friends: Phase 1 interpolation over the script's current
/// variables, bounded by the script's memory limit and deadline.
pub(crate) fn interpolate_with_layers(
    text: &str,
    layers_json: &str,
    limits: &InterpolateLimits,
) -> Result<String, InterpolateError> {
    let layers: Layers = serde_json::from_str(layers_json).unwrap_or_default();
    VarStack::from_values(
        layers.runtime,
        layers.request,
        vec![layers.folder],
        layers.environment,
        layers.collection,
    )
    .with_process_env(&layers.process_env)
    .try_interpolate(text, limits)
}
