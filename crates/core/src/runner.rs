//! Runs the requests under a target in Bruno's order and reports each one as a `RunEvent`.
//!
//! One request follows bruno-cli/src/runner/run-single-request.js: prepare (scopes merged,
//! nothing interpolated) → pre-request scripts → interpolate → send → post-response vars →
//! post-response scripts → asserts → tests. The run loop follows commands/run.js for
//! `skipRequest`, `stopExecution` and `setNextRequest`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use std::collections::BTreeSet;

use crate::bru::model::{RequestFile, ScopeFile};
use crate::collection::{Collection, Environment, Item, RequestRef};
use crate::executor::{ExecError, ExecOptions, ResponseSummary, Session};
use crate::persist::{self, VarChanges};
use crate::request::{HttpRequest, KeyValue, Method, MultipartField};
use crate::resolve::{
    PreparedBody, PreparedRequest, ResolveContext, ResolveError, ResolvedRequest,
    environment_values, finalize, initial_vars, prepare,
};
use crate::script::{
    AssertResult, ConsoleLine, Flow, NextRequest, ScriptBody, ScriptContext, ScriptEngine,
    ScriptError, ScriptInfo, ScriptLimits, ScriptRequest, ScriptResponse, ScriptVars, Segment,
    TestResult, run_asserts, run_post_response_vars, run_script_with_host,
};
use crate::session_host::{ScriptedRequest, SessionHost};
use crate::store::{self, StoreError};
use crate::vars::{
    InterpolateLimits, MAX_INTERPOLATED_BYTES, ValueMap, VarMap, enabled_map, string_values,
};
use crate::workspace::OpenCollection;

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub env: Option<String>,
    /// `--env-var` values, secrets and remembered script changes; they win over the
    /// environment file.
    pub env_overrides: ValueMap,
    pub exec: ExecOptions,
    pub scripts: ScriptLimits,
    pub persist: Persist,
    /// `--delay`: wait this long after every request except the last
    /// (Bruno commands/run.js), e.g. to stay under a server's rate limit.
    pub delay: Option<Duration>,
}

/// Which script changes to variables are written back to disk after each request
/// (Bruno 4.3: bruno-cli/src/utils/persist-variables.js).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Persist {
    /// Keep them in memory only.
    #[default]
    Nothing,
    /// The selected environment file, like the Bruno app.
    Environment,
    /// The environment file and `collection.bru` variables, like Bruno CLI.
    All,
}

/// Writes what one request's scripts changed, under the store's file lock and with minimal
/// diffs. Like Bruno, a failure only warns. `true` when a file changed.
pub fn persist_changes(
    collection: &Collection,
    environment: Option<&Environment>,
    overrides: &ValueMap,
    persist: Persist,
    before: &RunVars,
    after: &RunVars,
) -> bool {
    let mut wrote = false;
    if persist == Persist::Nothing {
        return false;
    }
    // No environment selected means nothing to persist it to; a removed file is not recreated.
    if let Some(env) = environment.filter(|e| e.path.is_file()) {
        let changes = VarChanges::between(&before.environment, &after.environment);
        if !changes.is_empty() {
            let keep: BTreeSet<String> = overrides.keys().cloned().collect();
            wrote |= write_changes(&env.path, |src| {
                persist::apply_env_changes(src.unwrap_or_default(), &changes, &keep)
            });
        }
    }
    if persist == Persist::All {
        let changes = VarChanges::between(&before.collection, &after.collection);
        if !changes.is_empty() {
            wrote |= write_changes(&collection.root.join("collection.bru"), |src| {
                persist::apply_collection_var_changes(src.unwrap_or_default(), &changes)
            });
        }
    }
    wrote
}

/// The entries of `vars` that are secrets of `environment` (their values stay in memory).
pub fn secret_values(environment: Option<&Environment>, vars: &ValueMap) -> ValueMap {
    let Some(env) = environment else {
        return ValueMap::new();
    };
    vars.iter()
        .filter(|(k, _)| env.file.secrets.iter().any(|s| &s.name == *k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn write_changes(
    path: &Path,
    apply: impl FnOnce(Option<&str>) -> Result<String, crate::bru::syntax::SyntaxError>,
) -> bool {
    match store::update_file(path, |src| apply(src).map_err(|e| e.to_string())) {
        Ok(wrote) => wrote,
        Err(e) => {
            tracing::warn!(
                "failed to persist variable updates to {}: {e}",
                path.display()
            );
            false
        }
    }
}

impl RunOptions {
    /// Adds string overrides, as the CLI's `--env-var KEY=VALUE` produces them.
    pub fn with_env_vars(mut self, vars: &VarMap) -> RunOptions {
        self.env_overrides.extend(string_values(vars));
        self
    }
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
        outcome: RequestOutcome,
    },
    /// A transport error, a script error, or a failed test or assert.
    Failed {
        path: PathBuf,
        /// The request's `meta.name`, or the file name when it has none.
        name: String,
        error: String,
        outcome: RequestOutcome,
    },
    Skipped {
        path: PathBuf,
        /// The request's `meta.name`, or the file name when it has none (unparsable file).
        name: String,
        reason: String,
    },
    /// Always the last event of a run.
    Finished { report: RunReport },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunReport {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub tests_passed: usize,
    pub tests_failed: usize,
    pub asserts_passed: usize,
    pub asserts_failed: usize,
    pub elapsed: Duration,
}

/// What happened to one request, apart from the response body.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RequestOutcome {
    /// `None` when the request was not sent or the transport failed.
    pub status: Option<u16>,
    /// The response time.
    pub elapsed: Duration,
    /// The whole request in a run, scripts included (Bruno's `runDuration`).
    pub duration: Duration,
    pub size: usize,
    pub tests: Vec<TestResult>,
    pub asserts: Vec<AssertResult>,
    pub console: Vec<ConsoleLine>,
    pub script_error: Option<ScriptFailure>,
    /// Requests scripts sent with `bru.sendRequest` or `axios`.
    pub scripted_requests: Vec<ScriptedRequest>,
}

impl RequestOutcome {
    /// Why a completed request counts as failed, if it does.
    fn failure_summary(&self) -> Option<String> {
        if let Some(f) = &self.script_error
            && f.phase != ScriptPhase::Tests
        {
            return Some(format!("{} script error: {}", f.phase, f.error));
        }
        let tests = self.tests.iter().filter(|t| !t.passed).count();
        let asserts = self.asserts.iter().filter(|a| !a.passed).count();
        let mut parts = Vec::new();
        if tests > 0 {
            parts.push(format!("{tests} test{} failed", plural(tests)));
        }
        if asserts > 0 {
            parts.push(format!("{asserts} assert{} failed", plural(asserts)));
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptFailure {
    pub phase: ScriptPhase,
    pub error: ScriptError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptPhase {
    PreRequest,
    PostResponse,
    Tests,
}

impl ScriptPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScriptPhase::PreRequest => "pre-request",
            ScriptPhase::PostResponse => "post-response",
            ScriptPhase::Tests => "tests",
        }
    }
}

impl std::fmt::Display for ScriptPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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

/// Variables carried from one request to the next.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunVars {
    pub runtime: ValueMap,
    /// The environment as the run sees it: file values, overrides, script changes.
    pub environment: ValueMap,
    /// Collection variables, including `bru.setCollectionVar` changes.
    pub collection: ValueMap,
}

impl RunVars {
    /// Starting state for `collection` with environment `env`.
    pub fn start(
        collection: &Collection,
        env: Option<&Environment>,
        env_overrides: &ValueMap,
        runtime: ValueMap,
    ) -> RunVars {
        let environment = environment_values(&ResolveContext {
            collection,
            environment: env,
            env_overrides,
            runtime: &runtime,
        });
        let collection_vars = collection
            .scope
            .as_ref()
            .map(|s| string_values(&enabled_map(&s.vars_pre)))
            .unwrap_or_default();
        RunVars {
            runtime,
            environment,
            collection: collection_vars,
        }
    }
}

/// Runs with a fresh session and empty runtime variables (the CLI).
pub async fn run(
    collection: &Collection,
    target: &Path,
    opts: &RunOptions,
    on_event: &mut (dyn FnMut(&RunEvent) + Send),
) -> Result<RunReport, RunError> {
    let session = Session::new(opts.exec.clone())?;
    let mut runtime = ValueMap::new();
    run_with(collection, target, opts, &session, &mut runtime, on_event).await
}

/// Runs with a caller-owned session (cookie jar) and runtime variables, so
/// a long-lived process can share them across runs and single sends.
/// `opts.exec` is ignored; the session's options apply.
pub async fn run_with(
    collection: &Collection,
    target: &Path,
    opts: &RunOptions,
    session: &Session,
    runtime: &mut ValueMap,
    on_event: &mut (dyn FnMut(&RunEvent) + Send),
) -> Result<RunReport, RunError> {
    let environment = find_environment(collection, opts.env.as_deref())?;
    let mut state = RunVars::start(
        collection,
        environment,
        &opts.env_overrides,
        std::mem::take(runtime),
    );
    let result = run_with_state(collection, target, opts, session, &mut state, on_event).await;
    *runtime = state.runtime;
    result
}

fn find_environment<'a>(
    collection: &'a Collection,
    name: Option<&str>,
) -> Result<Option<&'a Environment>, RunError> {
    match name {
        Some(name) => {
            Ok(Some(collection.environment(name).ok_or_else(|| {
                RunError::UnknownEnvironment(name.to_owned())
            })?))
        }
        None => Ok(None),
    }
}

/// Like `run_with`, but the caller also keeps the environment and collection variables
/// scripts changed (the server remembers them for the session).
pub async fn run_with_state(
    collection: &Collection,
    target: &Path,
    opts: &RunOptions,
    session: &Session,
    state: &mut RunVars,
    on_event: &mut (dyn FnMut(&RunEvent) + Send),
) -> Result<RunReport, RunError> {
    let environment = find_environment(collection, opts.env.as_deref())?;
    let absolute = std::path::absolute(target).unwrap_or_else(|_| target.to_path_buf());
    if !absolute.starts_with(&collection.root) || !absolute.exists() {
        return Err(RunError::TargetNotFound(target.to_path_buf()));
    }
    let refs = collection.requests_under(&absolute);
    let engine = ScriptEngine::shared(opts.scripts);
    let exec = Exec {
        collection,
        session,
        engine: &engine,
        env_name: opts.env.as_deref(),
        process_env: crate::dotenv::process_env(&collection.root),
    };

    let start = Instant::now();
    let mut report = RunReport::default();
    let mut index = 0;
    let mut jumps = 0;
    while index < refs.len() {
        let r = &refs[index];
        let path = r.path.to_path_buf();
        if let Err(not_runnable) = check_runnable(r) {
            match not_runnable {
                NotRunnable::Skip(reason) => {
                    report.skipped += 1;
                    on_event(&RunEvent::Skipped {
                        path,
                        name: display_name(r),
                        reason,
                    });
                }
                NotRunnable::Fail(e) => {
                    report.failed += 1;
                    on_event(&RunEvent::Failed {
                        path,
                        name: display_name(r),
                        error: e.to_string(),
                        outcome: RequestOutcome::default(),
                    });
                }
            }
            index += 1;
            continue;
        }
        let before = state.clone();
        let request_started = Instant::now();
        let mut executed = exec
            .execute(r, None, state, &mut |name, resolved| {
                on_event(&RunEvent::Started {
                    path: path.clone(),
                    name: name.to_owned(),
                    method: resolved.method.as_str().to_owned(),
                    url: resolved.url.clone(),
                })
            })
            .await;
        executed.outcome.duration = request_started.elapsed();
        persist_changes(
            collection,
            environment,
            &opts.env_overrides,
            opts.persist,
            &before,
            state,
        );
        let outcome = &executed.outcome;
        report.tests_passed += outcome.tests.iter().filter(|t| t.passed).count();
        report.tests_failed += outcome.tests.iter().filter(|t| !t.passed).count();
        report.asserts_passed += outcome.asserts.iter().filter(|a| a.passed).count();
        report.asserts_failed += outcome.asserts.iter().filter(|a| !a.passed).count();
        match (&executed.skipped, executed.error()) {
            (Some(reason), _) => {
                report.skipped += 1;
                on_event(&RunEvent::Skipped {
                    path,
                    name: display_name(r),
                    reason: reason.clone(),
                });
            }
            (None, Some(error)) => {
                report.failed += 1;
                on_event(&RunEvent::Failed {
                    path,
                    name: display_name(r),
                    error,
                    outcome: executed.outcome.clone(),
                });
            }
            (None, None) => {
                report.passed += 1;
                on_event(&RunEvent::Passed {
                    path,
                    outcome: executed.outcome.clone(),
                });
            }
        }

        // commands/run.js waits right after each request, before stop/jump handling.
        if let Some(delay) = opts.delay.filter(|d| !d.is_zero())
            && index + 1 < refs.len()
        {
            tokio::time::sleep(delay).await;
        }

        // commands/run.js: stopExecution skips the rest; setNextRequest jumps by name.
        if executed.flow.stop_execution {
            for rest in &refs[index + 1..] {
                report.skipped += 1;
                on_event(&RunEvent::Skipped {
                    path: rest.path.to_path_buf(),
                    name: display_name(rest),
                    reason: "stopExecution".to_owned(),
                });
            }
            break;
        }
        match &executed.flow.next_request {
            None => index += 1,
            Some(NextRequest::Stop) => break,
            Some(NextRequest::Name(name)) => {
                jumps += 1;
                if jumps > 10_000 {
                    report.failed += 1;
                    on_event(&RunEvent::Failed {
                        path: r.path.to_path_buf(),
                        name: display_name(r),
                        error: "too many jumps, possible infinite loop".to_owned(),
                        outcome: RequestOutcome::default(),
                    });
                    break;
                }
                match refs.iter().position(|x| request_name(x) == Some(name)) {
                    Some(next) => index = next,
                    None => {
                        tracing::warn!("could not find request with name '{name}'");
                        index += 1;
                    }
                }
            }
        }
    }
    report.elapsed = start.elapsed();
    on_event(&RunEvent::Finished { report });
    Ok(report)
}

fn display_name(r: &RequestRef<'_>) -> String {
    request_name(r).map(str::to_owned).unwrap_or_else(|| {
        r.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

fn request_name<'a>(r: &RequestRef<'a>) -> Option<&'a str> {
    match r.item {
        Item::Request { file, .. } => Some(file.request.name.as_str()),
        _ => None,
    }
}

/// Why a request was not sent: skipped like Bruno does, or failed to resolve.
enum NotRunnable {
    Skip(String),
    Fail(ResolveError),
}

fn check_runnable(r: &RequestRef<'_>) -> Result<(), NotRunnable> {
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
    Ok(())
}

/// Everything one request needs that does not change between requests.
struct Exec<'a> {
    collection: &'a Collection,
    session: &'a Session,
    engine: &'a ScriptEngine,
    env_name: Option<&'a str>,
    /// The process environment and `.env` (`{{process.env.NAME}}`, `bru.getProcessEnv`).
    process_env: ValueMap,
}

struct Executed {
    outcome: RequestOutcome,
    /// The response as received, with a body replaced by `res.setBody`.
    response: Option<ResponseSummary>,
    /// Set when the request did not complete: resolve, pre-request or transport failure.
    stopped: Option<Stopped>,
    skipped: Option<String>,
    flow: Flow,
}

enum Stopped {
    Resolve(ResolveError),
    Script,
    Transport(ExecError),
}

impl Executed {
    /// The failure message, when the request failed.
    fn error(&self) -> Option<String> {
        match &self.stopped {
            Some(Stopped::Resolve(e)) => Some(e.to_string()),
            Some(Stopped::Transport(e)) => Some(e.to_string()),
            Some(Stopped::Script) | None => self.outcome.failure_summary(),
        }
    }
}

/// Bruno's script flow (`bruno.json` `scripts.flow`): `sandwich` (default) runs
/// post-response scripts and tests from the request outwards.
fn sequential_flow(collection: &Collection) -> bool {
    collection
        .config
        .extra
        .get("scripts")
        .and_then(|s| s.get("flow"))
        .and_then(|f| f.as_str())
        == Some("sequential")
}

impl Exec<'_> {
    /// Script segments for one phase: collection, folders (outermost first), request —
    /// reversed for post-response scripts and tests in the sandwich flow
    /// (bruno-cli/src/utils/collection.js `mergeScripts`).
    fn segments(
        &self,
        r: &RequestRef<'_>,
        request_source: &Option<String>,
        pick: fn(&ScopeFile) -> &Option<String>,
        outward: bool,
    ) -> Vec<Segment> {
        let mut segments = Vec::new();
        let mut push = |scope: String, source: &Option<String>| {
            if let Some(src) = source.as_ref().filter(|s| !s.trim().is_empty()) {
                segments.push(Segment::new(scope, src.clone()));
            }
        };
        if let Some(scope) = &self.collection.scope {
            push("collection".to_owned(), pick(scope));
        }
        for folder in &r.folders {
            if let Some(scope) = &folder.scope {
                push(format!("folder {}", folder.name), pick(scope));
            }
        }
        push("request".to_owned(), request_source);
        if outward && !sequential_flow(self.collection) {
            segments.reverse();
        }
        segments
    }

    async fn execute(
        &self,
        r: &RequestRef<'_>,
        draft: Option<&HttpRequest>,
        state: &mut RunVars,
        started: &mut (dyn FnMut(&str, &ResolvedRequest) + Send),
    ) -> Executed {
        let mut executed = Executed {
            outcome: RequestOutcome::default(),
            response: None,
            stopped: None,
            skipped: None,
            flow: Flow::default(),
        };
        let draft_item;
        let r = match (draft, r.item) {
            (Some(draft), Item::Request { path, file }) => {
                draft_item = Item::Request {
                    path: path.clone(),
                    file: Box::new(RequestFile {
                        doc: file.doc.clone(),
                        request: draft.clone(),
                    }),
                };
                RequestRef {
                    item: &draft_item,
                    path: r.path,
                    folders: r.folders.clone(),
                }
            }
            _ => r.clone(),
        };
        let Item::Request { file, .. } = r.item else {
            executed.stopped = Some(Stopped::Resolve(ResolveError::NotARequest(
                r.path.to_path_buf(),
            )));
            return executed;
        };
        let request = &file.request;
        let ctx = ResolveContext {
            collection: self.collection,
            environment: None,
            env_overrides: &state.environment,
            runtime: &state.runtime,
        };
        let prepared = prepare(&ctx, &r).and_then(|p| Ok((p, initial_vars(&ctx, &r)?)));
        let (mut prepared, mut vars) = match prepared {
            Ok(both) => both,
            Err(e) => {
                executed.stopped = Some(Stopped::Resolve(e));
                return executed;
            }
        };
        vars.collection = state.collection.clone();
        let info = ScriptInfo {
            env_name: self.env_name.map(str::to_owned),
            collection_name: self.collection.config.name.clone(),
            collection_path: self.collection.root.to_string_lossy().into_owned(),
            request_name: request.name.clone(),
            process_env: self.process_env.clone(),
        };

        let host = SessionHost::new(self.session);
        // Pre-request scripts see the prepared, not yet interpolated request.
        let pre = self.segments(&r, &request.script_pre, |s| &s.script_pre, false);
        if !pre.is_empty() {
            let out = run_script_with_host(
                self.engine,
                &ScriptContext {
                    vars: vars.clone(),
                    info: info.clone(),
                    req: script_request(&prepared),
                    res: None,
                },
                pre,
                &host,
            )
            .await;
            executed
                .outcome
                .scripted_requests
                .extend(host.take_recorded());
            vars = out.vars;
            apply_script_request(&mut prepared, &out.req);
            merge_flow(&mut executed.flow, &out.flow);
            executed.outcome.console.extend(out.console);
            executed.outcome.tests.extend(out.tests);
            if let Some(error) = out.error {
                executed.outcome.script_error = Some(ScriptFailure {
                    phase: ScriptPhase::PreRequest,
                    error,
                });
                executed.stopped = Some(Stopped::Script);
                save(state, &vars);
                return executed;
            }
            if out.flow.skip_request {
                executed.skipped = Some("request skipped via pre-request script".to_owned());
                save(state, &vars);
                return executed;
            }
        }

        let limits = InterpolateLimits {
            max_bytes: MAX_INTERPOLATED_BYTES,
            deadline: Some(Instant::now() + self.engine.limits().timeout),
        };
        let resolved = match finalize(
            &prepared,
            &vars.var_stack().with_process_env(&self.process_env),
            &self.collection.root,
            &limits,
        ) {
            Ok(resolved) => resolved,
            Err(e) => {
                executed.stopped = Some(Stopped::Resolve(e));
                save(state, &vars);
                return executed;
            }
        };
        started(&request.name, &resolved);
        let mut response = match self.session.send(&resolved).await {
            Ok(response) => response,
            Err(e) => {
                // run-single-request.js returns before post-response vars, scripts and tests.
                executed.stopped = Some(Stopped::Transport(e));
                save(state, &vars);
                return executed;
            }
        };
        executed.outcome.status = Some(response.status);
        executed.outcome.elapsed = response.elapsed;
        executed.outcome.size = response.size;

        let mut script_ctx = ScriptContext {
            vars,
            info,
            req: script_request(&prepared),
            res: Some(script_response(&response, &resolved.url)),
        };

        let post_vars = enabled_pairs(&request.vars_post);
        if !post_vars.is_empty() {
            let out = run_post_response_vars(self.engine, &script_ctx, &post_vars).await;
            script_ctx.vars = out.vars;
            executed.outcome.console.extend(out.console);
        }

        let post = self.segments(&r, &request.script_post, |s| &s.script_post, true);
        if !post.is_empty() {
            let out = run_script_with_host(self.engine, &script_ctx, post, &host).await;
            executed
                .outcome
                .scripted_requests
                .extend(host.take_recorded());
            script_ctx.vars = out.vars;
            merge_flow(&mut executed.flow, &out.flow);
            executed.outcome.console.extend(out.console);
            executed.outcome.tests.extend(out.tests);
            if let Some(body) = out.res_body {
                replace_body(&mut response, &mut script_ctx, &body);
            }
            if let Some(error) = out.error {
                executed.outcome.script_error = Some(ScriptFailure {
                    phase: ScriptPhase::PostResponse,
                    error,
                });
            }
        }

        let asserts = enabled_pairs(&request.asserts);
        if !asserts.is_empty() {
            let out = run_asserts(self.engine, &script_ctx, &asserts).await;
            script_ctx.vars = out.vars;
            executed.outcome.console.extend(out.console);
            executed.outcome.asserts = out.asserts;
        }

        let tests = self.segments(&r, &request.tests, |s| &s.tests, true);
        if !tests.is_empty() {
            let out = run_script_with_host(self.engine, &script_ctx, tests, &host).await;
            executed
                .outcome
                .scripted_requests
                .extend(host.take_recorded());
            script_ctx.vars = out.vars;
            merge_flow(&mut executed.flow, &out.flow);
            executed.outcome.console.extend(out.console);
            executed.outcome.tests.extend(out.tests);
            if let Some(body) = out.res_body {
                replace_body(&mut response, &mut script_ctx, &body);
            }
            if let Some(error) = out.error {
                // run-single-request.js: a test script error becomes a failed test.
                executed.outcome.tests.push(TestResult {
                    name: "Test Script Error".to_owned(),
                    passed: false,
                    error: Some(error.to_string()),
                });
                if executed.outcome.script_error.is_none() {
                    executed.outcome.script_error = Some(ScriptFailure {
                        phase: ScriptPhase::Tests,
                        error,
                    });
                }
            }
        }

        save(state, &script_ctx.vars);
        executed.response = Some(response);
        executed
    }
}

fn save(state: &mut RunVars, vars: &ScriptVars) {
    state.runtime = vars.runtime.clone();
    state.environment = vars.environment.clone();
    state.collection = vars.collection.clone();
}

fn merge_flow(flow: &mut Flow, new: &Flow) {
    flow.skip_request |= new.skip_request;
    flow.stop_execution |= new.stop_execution;
    if new.next_request.is_some() {
        flow.next_request = new.next_request.clone();
    }
}

fn enabled_pairs(items: &[KeyValue]) -> Vec<(String, String)> {
    items
        .iter()
        .filter(|kv| kv.enabled)
        .map(|kv| (kv.name.clone(), kv.value.clone()))
        .collect()
}

fn script_request(p: &PreparedRequest) -> ScriptRequest {
    ScriptRequest {
        url: p.url.clone(),
        method: p.method.as_str().to_owned(),
        headers: p.headers.clone(),
        body: match &p.body {
            PreparedBody::None => ScriptBody::None,
            PreparedBody::Text(text) => ScriptBody::Text { text: text.clone() },
            PreparedBody::Form(fields) => ScriptBody::Form {
                fields: fields.clone(),
            },
            PreparedBody::Multipart(_) => ScriptBody::Multipart,
        },
        timeout_ms: p.settings.timeout_ms,
        max_redirects: p.settings.max_redirects,
    }
}

fn apply_script_request(p: &mut PreparedRequest, s: &ScriptRequest) {
    p.url = s.url.clone();
    if s.method != p.method.as_str() {
        p.method = Method::from_block_name(&s.method.to_ascii_lowercase())
            .unwrap_or_else(|| Method::Custom(s.method.clone()));
    }
    p.headers = s.headers.clone();
    let multipart: Vec<MultipartField> = match &p.body {
        PreparedBody::Multipart(fields) => fields.clone(),
        _ => Vec::new(),
    };
    p.body = match &s.body {
        ScriptBody::None => PreparedBody::None,
        ScriptBody::Text { text } => PreparedBody::Text(text.clone()),
        ScriptBody::Form { fields } => PreparedBody::Form(fields.clone()),
        ScriptBody::Multipart => PreparedBody::Multipart(multipart),
    };
    p.settings.timeout_ms = s.timeout_ms;
    p.settings.max_redirects = s.max_redirects;
}

fn script_response(res: &ResponseSummary, url: &str) -> ScriptResponse {
    ScriptResponse {
        status: res.status,
        status_text: reqwest::StatusCode::from_u16(res.status)
            .ok()
            .and_then(|s| s.canonical_reason())
            .unwrap_or_default()
            .to_owned(),
        headers: res.headers.clone(),
        body: String::from_utf8_lossy(&res.body).into_owned(),
        response_time_ms: res.elapsed.as_millis() as u64,
        url: url.to_owned(),
        size: res.size,
    }
}

/// `res.setBody(value)`: later phases and the reported response see the new body.
fn replace_body(response: &mut ResponseSummary, ctx: &mut ScriptContext, body: &serde_json::Value) {
    let text = match body {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    };
    response.body = text.clone().into_bytes();
    if let Some(res) = ctx.res.as_mut() {
        res.body = text;
        res.size = response.body.len();
    }
}

pub struct SendInput<'a> {
    /// `/`-separated path of the request file relative to the collection root.
    pub item_id: &'a str,
    /// Unsaved edits to send instead of the file's request, with the file's scopes.
    pub draft: Option<&'a HttpRequest>,
    pub env: Option<&'a str>,
}

/// One request sent from the UI: the response (if any) and what scripts reported.
#[derive(Debug, Clone)]
pub struct SendOutcome {
    pub response: Option<ResponseSummary>,
    pub outcome: RequestOutcome,
    /// Set when the pre-request script skipped the request.
    pub skipped: Option<String>,
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

/// Sends one request of an open collection with its session, runtime variables,
/// in-memory secrets and remembered script changes to the environment. A pre-request
/// script error is an `Ok` outcome without a response (`outcome.script_error`).
pub async fn send_one(
    open: &OpenCollection,
    input: SendInput<'_>,
) -> Result<SendOutcome, SendError> {
    send_one_with(open, input, ScriptLimits::default()).await
}

pub async fn send_one_with(
    open: &OpenCollection,
    input: SendInput<'_>,
    limits: ScriptLimits,
) -> Result<SendOutcome, SendError> {
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
    if let Err(not_runnable) = check_runnable(&found) {
        return Err(match not_runnable {
            NotRunnable::Skip(reason) => SendError::Unsupported(reason),
            NotRunnable::Fail(e) => SendError::Resolve(e),
        });
    }
    let overrides = input.env.map(|e| open.env_overrides(e)).unwrap_or_default();
    let runtime_before = open.runtime();
    let mut state = RunVars::start(&collection, environment, &overrides, runtime_before.clone());
    let before = state.clone();
    let engine = ScriptEngine::shared(limits);
    let exec = Exec {
        collection: &collection,
        session: &open.session,
        engine: &engine,
        env_name: input.env,
        process_env: crate::dotenv::process_env(&collection.root),
    };
    let executed = exec
        .execute(&found, input.draft, &mut state, &mut |_, _| {})
        .await;
    // Like the Bruno app, script changes to the environment go to its file; secrets stay
    // in memory.
    let persisted = persist_changes(
        &collection,
        environment,
        &overrides,
        Persist::Environment,
        &before,
        &state,
    );
    open.record_runtime_changes(&runtime_before, &state.runtime);
    if let Some(env) = input.env {
        if persisted {
            open.record_env_changes(
                env,
                &secret_values(environment, &before.environment),
                &secret_values(environment, &state.environment),
            );
        } else {
            open.record_env_changes(env, &before.environment, &state.environment);
        }
    }
    if persisted && let Err(e) = open.reload() {
        tracing::warn!("cannot reload the collection after saving variables: {e}");
    }
    match executed.stopped {
        Some(Stopped::Resolve(ResolveError::Unsupported(what))) => {
            Err(SendError::Unsupported(format!("unsupported {what}")))
        }
        Some(Stopped::Resolve(e)) => Err(SendError::Resolve(e)),
        Some(Stopped::Transport(e)) => Err(SendError::Exec(e)),
        Some(Stopped::Script) | None => Ok(SendOutcome {
            response: executed.response,
            outcome: executed.outcome,
            skipped: executed.skipped,
        }),
    }
}
