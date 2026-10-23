//! A pool of QuickJS runtimes that run scripts in fresh, sandboxed contexts.
//!
//! Each worker thread owns one `rquickjs::Runtime` (memory and stack limits, an interrupt
//! handler for the timeout) and the safe-mode libraries compiled to bytecode once. A job
//! gets a new context: libraries, `prelude.js`, `api.js`, then the job's code. The script is
//! driven to completion on the worker: pending promise jobs first, then timers, sleeping
//! until the next one is due. Scripts only see what the prelude installs; QuickJS without
//! quickjs-libc has no filesystem, network or process access.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use rquickjs::context::EvalOptions;
use rquickjs::module::WriteOptions;
use rquickjs::{CaughtError, Context, Ctx, Function, Module, Runtime};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use super::libs::LIBS;
use crate::vars::InterpolateLimits;

const PRELUDE_JS: &str = include_str!("prelude.js");
const API_JS: &str = include_str!("api.js");
/// File name the user's code is evaluated under; stack frames are matched against it.
const SCRIPT_FILE: &str = "bru-script";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptLimits {
    /// Wall clock for one script phase, including awaited timers and promises.
    pub timeout: Duration,
    /// Heap limit of a worker runtime; a script that needs more fails.
    pub memory_bytes: usize,
    pub max_stack_bytes: usize,
}

impl Default for ScriptLimits {
    fn default() -> Self {
        ScriptLimits {
            timeout: Duration::from_secs(30),
            memory_bytes: 128 * 1024 * 1024,
            max_stack_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Log,
    Info,
    Warn,
    Error,
    Debug,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleLine {
    pub level: LogLevel,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScriptError {
    #[error("{}", thrown_display(.name, .message, .scope, .line))]
    Thrown {
        name: String,
        message: String,
        /// Which segment threw: `collection`, `folder <name>` or `request`.
        scope: Option<String>,
        /// 1-based line within that segment's source.
        line: Option<u32>,
        column: Option<u32>,
        stack: Option<String>,
    },
    #[error("{message}")]
    ForbiddenModule {
        module_name: String,
        message: String,
    },
    #[error("script timed out after {} ms", .0.as_millis())]
    Timeout(Duration),
    #[error("script exceeded the memory limit")]
    OutOfMemory,
    #[error("script awaited a promise that can never settle")]
    Unsettled,
    /// The script ran, but what it produced (variables, tests, console output, request
    /// changes) could not be handed back, so none of it is trusted.
    #[error("could not report script results: {0}")]
    ResultUnavailable(String),
    #[error("script engine error: {0}")]
    Internal(String),
}

fn thrown_display(name: &str, message: &str, scope: &Option<String>, line: &Option<u32>) -> String {
    let mut s = format!("{name}: {message}");
    match (scope, line) {
        (Some(scope), Some(line)) => s.push_str(&format!(" ({scope}, line {line})")),
        (Some(scope), None) => s.push_str(&format!(" ({scope})")),
        _ => {}
    }
    s
}

/// One script segment: the code of one scope, run in its own async closure like Bruno's
/// `wrapScriptInClosure` (bruno-cli/src/utils/collection.js:232-243).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub scope: String,
    pub source: String,
}

impl Segment {
    pub fn new(scope: impl Into<String>, source: impl Into<String>) -> Segment {
        Segment {
            scope: scope.into(),
            source: source.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Main {
    /// User scripts, run in order in one context.
    Segments(Vec<Segment>),
    /// A call into the prelude/API (asserts, post-response vars); must return a promise or value.
    Call(String),
}

#[derive(Debug, Clone)]
pub struct EngineJob {
    /// Available to `api.js` as `__plv.input`.
    pub input: serde_json::Value,
    pub main: Main,
    /// Where `__plvHost` sends operations the worker can't do itself (HTTP, cookies);
    /// `None` makes them throw.
    pub host: Option<HostSender>,
}

/// A host operation from a script, answered by the runner that owns the session.
#[derive(Debug)]
pub struct HostRequest {
    pub op: String,
    pub args: serde_json::Value,
    /// The script phase's deadline; answer before it or the script gives up.
    pub deadline: Instant,
    pub reply: mpsc::Sender<Result<serde_json::Value, String>>,
}

pub type HostSender = tokio::sync::mpsc::UnboundedSender<HostRequest>;

#[derive(Debug, Clone, Default)]
pub struct EngineOutput {
    /// Everything `api.js` reported besides console output and the error (state, tests, …).
    pub output: serde_json::Map<String, serde_json::Value>,
    pub console: Vec<ConsoleLine>,
    pub error: Option<ScriptError>,
}

/// Cheap to clone; worker threads start on the first `run`.
#[derive(Clone)]
pub struct ScriptEngine {
    inner: Arc<Inner>,
}

struct Inner {
    limits: ScriptLimits,
    queue: OnceLock<Mutex<mpsc::Sender<Job>>>,
}

struct Job {
    job: EngineJob,
    reply: oneshot::Sender<EngineOutput>,
    /// Set when the caller stops waiting (its future was dropped): the worker skips or
    /// interrupts the job.
    cancelled: Arc<AtomicBool>,
}

/// Marks the job cancelled when `ScriptEngine::run`'s future is dropped before it ends.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl ScriptEngine {
    pub fn new(limits: ScriptLimits) -> ScriptEngine {
        ScriptEngine {
            inner: Arc::new(Inner {
                limits,
                queue: OnceLock::new(),
            }),
        }
    }

    /// One engine per distinct limits for the whole process, so runs and sends reuse warm
    /// workers instead of starting threads each time.
    pub fn shared(limits: ScriptLimits) -> ScriptEngine {
        static ENGINES: OnceLock<Mutex<Vec<ScriptEngine>>> = OnceLock::new();
        let mut engines = ENGINES
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(engine) = engines.iter().find(|e| e.limits() == limits) {
            return engine.clone();
        }
        let engine = ScriptEngine::new(limits);
        engines.push(engine.clone());
        engine
    }

    pub fn limits(&self) -> ScriptLimits {
        self.inner.limits
    }

    pub async fn run(&self, job: EngineJob) -> EngineOutput {
        let (reply, rx) = oneshot::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let guard = CancelOnDrop(Arc::clone(&cancelled));
        let sent = self
            .sender()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(Job {
                job,
                reply,
                cancelled,
            });
        if sent.is_err() {
            return internal("script workers are not running");
        }
        let output = rx
            .await
            .unwrap_or_else(|_| internal("script worker stopped unexpectedly"));
        // Finished normally: nothing to cancel.
        std::mem::forget(guard);
        output
    }

    fn sender(&self) -> &Mutex<mpsc::Sender<Job>> {
        self.inner.queue.get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            let rx = Arc::new(Mutex::new(rx));
            let workers = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                .clamp(1, 4);
            for i in 0..workers {
                let rx = Arc::clone(&rx);
                let limits = self.inner.limits;
                let spawned = std::thread::Builder::new()
                    .name(format!("plover-script-{i}"))
                    // Room for QuickJS's own stack limit plus the Rust frames around it.
                    .stack_size(limits.max_stack_bytes + 8 * 1024 * 1024)
                    .spawn(move || worker_loop(limits, &rx));
                if let Err(e) = spawned {
                    tracing::error!("cannot start script worker: {e}");
                }
            }
            Mutex::new(tx)
        })
    }
}

fn internal(message: &str) -> EngineOutput {
    EngineOutput {
        error: Some(ScriptError::Internal(message.to_owned())),
        ..EngineOutput::default()
    }
}

fn worker_loop(limits: ScriptLimits, rx: &Mutex<mpsc::Receiver<Job>>) {
    let mut worker: Option<Worker> = None;
    loop {
        let job = {
            let rx = rx.lock().unwrap_or_else(PoisonError::into_inner);
            rx.recv()
        };
        let Ok(Job {
            job,
            reply,
            cancelled,
        }) = job
        else {
            return;
        };
        if cancelled.load(Ordering::Relaxed) {
            continue;
        }
        if worker.is_none() {
            match Worker::new(limits) {
                Ok(w) => worker = Some(w),
                Err(e) => {
                    let _ = reply.send(internal(&format!("cannot start QuickJS: {e}")));
                    continue;
                }
            }
        }
        let Some(w) = worker.as_ref() else { continue };
        let output = catch_unwind(AssertUnwindSafe(|| w.execute(job, cancelled)));
        let output = output.unwrap_or_else(|_| {
            // Start over with a fresh runtime after a panic.
            worker = None;
            internal("script engine panicked")
        });
        let _ = reply.send(output);
    }
}

struct Worker {
    runtime: Runtime,
    limits: ScriptLimits,
    deadline: Rc<Cell<Option<Instant>>>,
    /// The running job's cancel flag, read by the interrupt handler.
    cancelled: Rc<RefCell<Arc<AtomicBool>>>,
    /// Module bytecode of each library from `LIBS`, compiled on first use.
    libs: Rc<RefCell<HashMap<&'static str, Vec<u8>>>>,
}

impl Worker {
    fn new(limits: ScriptLimits) -> Result<Worker, String> {
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        runtime.set_memory_limit(limits.memory_bytes);
        runtime.set_max_stack_size(limits.max_stack_bytes);
        let deadline: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
        let cancelled: Rc<RefCell<Arc<AtomicBool>>> = Rc::default();
        let watched = Rc::clone(&deadline);
        let watched_cancel = Rc::clone(&cancelled);
        runtime.set_interrupt_handler(Some(Box::new(move || {
            watched.get().is_some_and(|d| Instant::now() >= d)
                || watched_cancel.borrow().load(Ordering::Relaxed)
        })));
        Ok(Worker {
            runtime,
            limits,
            deadline,
            libs: Rc::default(),
            cancelled,
        })
    }

    fn execute(&self, job: EngineJob, cancelled: Arc<AtomicBool>) -> EngineOutput {
        *self.cancelled.borrow_mut() = cancelled;
        let started = Instant::now();
        let deadline = started + self.limits.timeout;
        self.deadline.set(Some(deadline));
        let output = self.execute_in_context(job, started, deadline);
        self.deadline.set(None);
        *self.cancelled.borrow_mut() = Arc::default();
        output
    }

    fn execute_in_context(
        &self,
        job: EngineJob,
        started: Instant,
        deadline: Instant,
    ) -> EngineOutput {
        let context = match Context::full(&self.runtime) {
            Ok(c) => c,
            Err(e) => return internal(&format!("cannot create a script context: {e}")),
        };
        let (code, segments) = build_main(&job.main);
        let input_json = job.input.to_string();
        let host = job.host;
        let cancelled = Arc::clone(&self.cancelled.borrow());

        let setup = context.with(|ctx| -> Result<(), ScriptError> {
            let globals = ctx.globals();
            let internal_err = |e: rquickjs::Error| ScriptError::Internal(e.to_string());
            globals
                .set("__plvInputJson", input_json.as_str())
                .map_err(internal_err)?;
            let now = Function::new(ctx.clone(), move || {
                started.elapsed().as_secs_f64() * 1000.0
            })
            .map_err(internal_err)?;
            globals.set("__plvNow", now).map_err(internal_err)?;
            let max_bytes = self.limits.memory_bytes;
            let interpolate = Function::new(
                ctx.clone(),
                move |ctx: Ctx<'_>, text: String, layers: String| {
                    let limits = InterpolateLimits {
                        max_bytes,
                        deadline: Some(deadline),
                    };
                    super::api::interpolate_with_layers(&text, &layers, &limits)
                        .map_err(|e| rquickjs::Exception::throw_message(&ctx, &e.to_string()))
                },
            )
            .map_err(internal_err)?;
            globals
                .set("__plvInterpolate", interpolate)
                .map_err(internal_err)?;
            let libs = Rc::clone(&self.libs);
            let load_lib = Function::new(ctx.clone(), move |ctx: Ctx<'_>, name: String| {
                load_lib(&ctx, &libs, &name)
            })
            .map_err(internal_err)?;
            globals
                .set("__plvLoadLib", load_lib)
                .map_err(internal_err)?;
            let host_fn = Function::new(
                ctx.clone(),
                move |ctx: Ctx<'_>, op: String, args: String| -> rquickjs::Result<String> {
                    let args = serde_json::from_str(&args).unwrap_or(serde_json::Value::Null);
                    let result = match super::host::builtin(&op, &args) {
                        Some(result) => result.map_err(HostFailure::Message),
                        None => call_host(host.as_ref(), op, args, deadline, &cancelled),
                    };
                    match result {
                        Ok(value) => Ok(value.to_string()),
                        // `classify` turns this into a timeout once the deadline has passed.
                        Err(HostFailure::Interrupted) => {
                            Err(rquickjs::Exception::throw_message(&ctx, "interrupted"))
                        }
                        Err(HostFailure::Message(m)) => {
                            Err(rquickjs::Exception::throw_message(&ctx, &m))
                        }
                    }
                },
            )
            .map_err(internal_err)?;
            globals.set("__plvHost", host_fn).map_err(internal_err)?;
            // Compiled once per worker like the libraries: both are strict IIFEs over
            // globalThis, so module evaluation runs them unchanged.
            load_module(&ctx, &self.libs, "plv-prelude", PRELUDE_JS)
                .map_err(|e| self.classify(&ctx, e, deadline, &[]))?;
            load_module(&ctx, &self.libs, "plv-api", API_JS)
                .map_err(|e| self.classify(&ctx, e, deadline, &[]))?;
            eval_global(&ctx, &code, SCRIPT_FILE, false)
                .map_err(|e| self.classify(&ctx, e, deadline, &segments))?;
            Ok(())
        });

        let mut error = setup.err();
        if error.is_none() {
            error = self.drive(&context, deadline, &segments);
        }

        // Collect what the script produced, even after a failure. Collecting counts toward the
        // script's own budget (large results can take seconds on a slow machine), with at least
        // a short grace period so a timed-out script can still report its partial state.
        self.deadline.set(Some(std::cmp::max(
            deadline,
            Instant::now() + Duration::from_secs(2),
        )));
        let collected: Result<Vec<String>, String> = context.with(|ctx| {
            let f: Function = ctx
                .globals()
                .get("__plvResult")
                .map_err(|e| e.to_string())?;
            f.call::<_, Vec<String>>(())
                .map_err(|e| match CaughtError::from_error(&ctx, e) {
                    CaughtError::Exception(ex) => ex.message().unwrap_or_default(),
                    other => other.to_string(),
                })
        });
        let mut output = EngineOutput::default();
        let mut result_errors = Vec::new();
        match collected {
            Err(message) => result_errors.push(message),
            Ok(items) => {
                for pair in items.chunks(2) {
                    let [key, value] = pair else { continue };
                    if let Some(part) = key.strip_prefix('!') {
                        result_errors.push(format!("{}: {value}", part_label(part)));
                        continue;
                    }
                    let parsed = match serde_json::from_str::<serde_json::Value>(value) {
                        Ok(v) => v,
                        Err(e) => {
                            let reason = if e.to_string().contains("recursion limit") {
                                "a value is nested more than 128 levels deep".to_owned()
                            } else {
                                e.to_string()
                            };
                            result_errors.push(format!("{}: {reason}", part_label(key)));
                            continue;
                        }
                    };
                    match key.as_str() {
                        "console" => {
                            output.console = serde_json::from_value(parsed).unwrap_or_default()
                        }
                        "error" => {
                            if !parsed.is_null() && error.is_none() {
                                error = Some(match error_from_js(&parsed, &segments) {
                                    // A host call gave up at the deadline and the script let
                                    // the error through.
                                    ScriptError::Thrown { message, .. }
                                        if message == "interrupted"
                                            && Instant::now() >= deadline =>
                                    {
                                        ScriptError::Timeout(self.limits.timeout)
                                    }
                                    other => other,
                                });
                            }
                        }
                        _ => {
                            output.output.insert(key.clone(), parsed);
                        }
                    }
                }
            }
        }
        // Fail closed: results that could not all be handed back are not trusted.
        if error.is_none() && !result_errors.is_empty() {
            error = Some(if result_errors.iter().any(|e| is_out_of_memory(e)) {
                ScriptError::OutOfMemory
            } else {
                ScriptError::ResultUnavailable(result_errors.join("; "))
            });
        }
        output.error = error;
        output
    }

    /// Runs promise jobs and timers until the script reports done.
    fn drive(
        &self,
        context: &Context,
        deadline: Instant,
        segments: &[SegmentSpan],
    ) -> Option<ScriptError> {
        loop {
            loop {
                match self.runtime.execute_pending_job() {
                    Ok(true) => continue,
                    Ok(false) => break,
                    Err(job_error) => {
                        return Some(job_error.0.with(|ctx| {
                            self.classify(&ctx, rquickjs::Error::Exception, deadline, segments)
                        }));
                    }
                }
            }
            let done = context.with(|ctx| call_global::<bool>(&ctx, "__plvIsDone"));
            match done {
                Ok(true) => return None,
                Ok(false) => {}
                Err(e) => {
                    return Some(context.with(|ctx| self.classify(&ctx, e, deadline, segments)));
                }
            }
            if Instant::now() >= deadline {
                return Some(ScriptError::Timeout(self.limits.timeout));
            }
            let delay = match context.with(|ctx| call_global::<f64>(&ctx, "__plvNextTimerDelay")) {
                Ok(d) => d,
                Err(e) => {
                    return Some(context.with(|ctx| self.classify(&ctx, e, deadline, segments)));
                }
            };
            if delay < 0.0 {
                return Some(ScriptError::Unsettled);
            }
            let wake = Instant::now() + Duration::from_secs_f64(delay / 1000.0);
            if self.sleep_until(wake.min(deadline)) {
                return Some(ScriptError::Internal("script cancelled".to_owned()));
            }
            if wake >= deadline {
                return Some(ScriptError::Timeout(self.limits.timeout));
            }
            if let Err(e) = context.with(|ctx| call_global::<()>(&ctx, "__plvFireTimer")) {
                return Some(context.with(|ctx| self.classify(&ctx, e, deadline, segments)));
            }
        }
    }

    /// Sleeps until `until` in short slices; `true` if the job was cancelled meanwhile.
    fn sleep_until(&self, until: Instant) -> bool {
        loop {
            if self.cancelled.borrow().load(Ordering::Relaxed) {
                return true;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            std::thread::sleep(left.min(Duration::from_millis(20)));
        }
    }

    fn classify(
        &self,
        ctx: &Ctx<'_>,
        error: rquickjs::Error,
        deadline: Instant,
        segments: &[SegmentSpan],
    ) -> ScriptError {
        let caught = CaughtError::from_error(ctx, error);
        self.classify_caught(caught, deadline, segments)
    }

    fn classify_caught(
        &self,
        caught: CaughtError<'_>,
        deadline: Instant,
        segments: &[SegmentSpan],
    ) -> ScriptError {
        match caught {
            CaughtError::Error(rquickjs::Error::Allocation) => ScriptError::OutOfMemory,
            CaughtError::Error(e) => ScriptError::Internal(e.to_string()),
            CaughtError::Exception(ex) => {
                let message = ex.message().unwrap_or_default();
                if is_out_of_memory(&message) {
                    return ScriptError::OutOfMemory;
                }
                if message == "interrupted" && Instant::now() >= deadline {
                    return ScriptError::Timeout(self.limits.timeout);
                }
                let name: String = ex
                    .as_object()
                    .get("name")
                    .unwrap_or_else(|_| "Error".to_owned());
                let forbidden: Option<String> =
                    ex.as_object().get("__plvForbiddenModule").ok().flatten();
                error_from_parts(name, message, ex.stack(), forbidden, segments)
            }
            CaughtError::Value(v) => {
                let text = v
                    .as_string()
                    .and_then(|s| s.to_string().ok())
                    .unwrap_or_else(|| format!("{v:?}"));
                if is_out_of_memory(&text) {
                    return ScriptError::OutOfMemory;
                }
                if Instant::now() >= deadline {
                    return ScriptError::Timeout(self.limits.timeout);
                }
                error_from_parts("Error".to_owned(), text, None, None, segments)
            }
        }
    }
}

enum HostFailure {
    Interrupted,
    Message(String),
}

/// Sends `op` to the runner and blocks this worker until it answers, the phase's deadline
/// passes or the job is cancelled.
fn call_host(
    host: Option<&HostSender>,
    op: String,
    args: serde_json::Value,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<serde_json::Value, HostFailure> {
    let Some(host) = host else {
        return Err(HostFailure::Message(format!(
            "{op} is not available in this script phase"
        )));
    };
    let (reply, answer) = mpsc::channel();
    host.send(HostRequest {
        op,
        args,
        deadline,
        reply,
    })
    .map_err(|_| HostFailure::Message("the script host has stopped".to_owned()))?;
    loop {
        match answer.recv_timeout(Duration::from_millis(20)) {
            Ok(Err(message)) if message == "interrupted" => return Err(HostFailure::Interrupted),
            Ok(result) => return result.map_err(HostFailure::Message),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if cancelled.load(Ordering::Relaxed) || Instant::now() >= deadline {
                    return Err(HostFailure::Interrupted);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(HostFailure::Message(
                    "the script host has stopped".to_owned(),
                ));
            }
        }
    }
}

/// `__plvLoadLib(name)`: evaluates a library in the calling context, compiling it to bytecode
/// the first time this worker needs it. Library modules have no imports or top-level await,
/// so evaluation completes synchronously.
fn load_lib<'js>(
    ctx: &Ctx<'js>,
    cache: &RefCell<HashMap<&'static str, Vec<u8>>>,
    name: &str,
) -> rquickjs::Result<()> {
    let Some(&(name, source)) = LIBS.iter().find(|(n, _)| *n == name) else {
        return Err(rquickjs::Exception::throw_message(
            ctx,
            &format!("unknown library {name}"),
        ));
    };
    load_module(ctx, cache, name, source)
}

/// Evaluates `source` as a module in the calling context, from bytecode compiled the first
/// time this worker needs it. The modules have no imports or top-level await, so
/// evaluation completes synchronously.
fn load_module<'js>(
    ctx: &Ctx<'js>,
    cache: &RefCell<HashMap<&'static str, Vec<u8>>>,
    name: &'static str,
    source: &'static str,
) -> rquickjs::Result<()> {
    if !cache.borrow().contains_key(name) {
        let bytes = Module::declare(ctx.clone(), name, source)?.write(WriteOptions::default())?;
        cache.borrow_mut().insert(name, bytes);
    }
    let cache = cache.borrow();
    // SAFETY: the bytecode was produced by `Module::write` on this runtime's QuickJS build.
    let module = unsafe { Module::load(ctx.clone(), &cache[name]) }?;
    let (_, promise) = module.eval()?;
    match promise.result::<()>() {
        Some(Err(e)) => Err(e),
        _ => Ok(()),
    }
}

/// What a result part holds, for error messages.
fn part_label(part: &str) -> &str {
    match part {
        "vars" => "variables",
        "req" => "request changes",
        "tests" => "test results",
        "asserts" => "assert results",
        "console" => "console output",
        "resBody" => "response body",
        other => other,
    }
}

fn is_out_of_memory(message: &str) -> bool {
    message.contains("out of memory")
}

fn eval_global(ctx: &Ctx<'_>, source: &str, file: &str, strict: bool) -> rquickjs::Result<()> {
    let mut options = EvalOptions::default();
    options.global = true;
    options.strict = strict;
    options.filename = Some(file.to_owned());
    ctx.eval_with_options::<(), _>(source, options)
}

fn call_global<'js, T: rquickjs::FromJs<'js>>(ctx: &Ctx<'js>, name: &str) -> rquickjs::Result<T> {
    let f: Function = ctx.globals().get(name)?;
    f.call(())
}

/// Where a segment's code sits in the combined script (1-based, inclusive).
#[derive(Debug, Clone)]
struct SegmentSpan {
    scope: String,
    first_line: u32,
    last_line: u32,
}

/// Wraps each segment in an awaited async closure inside `__plvStart`, like Bruno's
/// `wrapAndJoinScripts`, and records where each one starts for error positions.
fn build_main(main: &Main) -> (String, Vec<SegmentSpan>) {
    match main {
        Main::Call(expr) => (
            format!("__plvStart(async () => {{ await ({expr}); }});\n"),
            Vec::new(),
        ),
        Main::Segments(segments) => {
            let mut code = String::from("__plvStart(async () => {\n");
            let mut line: u32 = 1;
            let mut spans = Vec::new();
            for segment in segments.iter().filter(|s| !s.source.trim().is_empty()) {
                code.push_str("await (async () => {\n");
                line += 1;
                let lines = segment.source.split('\n').count() as u32;
                spans.push(SegmentSpan {
                    scope: segment.scope.clone(),
                    first_line: line + 1,
                    last_line: line + lines,
                });
                code.push_str(&segment.source);
                code.push_str("\n})();\n");
                line += lines + 1;
            }
            code.push_str("});\n");
            (code, spans)
        }
    }
}

fn error_from_js(value: &serde_json::Value, segments: &[SegmentSpan]) -> ScriptError {
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    let message = text("message").unwrap_or_default();
    if is_out_of_memory(&message) {
        return ScriptError::OutOfMemory;
    }
    error_from_parts(
        text("name").unwrap_or_else(|| "Error".to_owned()),
        message,
        text("stack"),
        text("forbiddenModule"),
        segments,
    )
}

fn error_from_parts(
    name: String,
    message: String,
    stack: Option<String>,
    forbidden_module: Option<String>,
    segments: &[SegmentSpan],
) -> ScriptError {
    if let Some(module_name) = forbidden_module {
        return ScriptError::ForbiddenModule {
            module_name,
            message,
        };
    }
    let position = stack.as_deref().and_then(script_position);
    let (scope, line, column) = match position {
        Some((line, column)) => match segments
            .iter()
            .find(|s| (s.first_line..=s.last_line).contains(&line))
        {
            Some(s) => (Some(s.scope.clone()), Some(line - s.first_line + 1), column),
            None => (None, None, None),
        },
        None => (None, None, None),
    };
    ScriptError::Thrown {
        name,
        message,
        scope,
        line,
        column,
        stack,
    }
}

/// First `bru-script:LINE[:COL]` position in a QuickJS stack trace.
fn script_position(stack: &str) -> Option<(u32, Option<u32>)> {
    let marker = format!("{SCRIPT_FILE}:");
    let start = stack.find(&marker)? + marker.len();
    let rest = &stack[start..];
    let line_end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let line: u32 = rest[..line_end].parse().ok()?;
    let column = rest[line_end..].strip_prefix(':').and_then(|r| {
        let end = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
        r[..end].parse().ok()
    });
    Some((line, column))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_spans_match_wrapping() {
        let (code, spans) = build_main(&Main::Segments(vec![
            Segment::new("collection", "a\nb"),
            Segment::new("folder f", "   "),
            Segment::new("request", "c"),
        ]));
        let lines: Vec<&str> = code.lines().collect();
        assert_eq!(spans.len(), 2);
        assert_eq!(lines[spans[0].first_line as usize - 1], "a");
        assert_eq!(lines[spans[0].last_line as usize - 1], "b");
        assert_eq!(lines[spans[1].first_line as usize - 1], "c");
    }

    #[test]
    fn parses_stack_positions() {
        assert_eq!(
            script_position("    at <anonymous> (bru-script:12:5)\n"),
            Some((12, Some(5)))
        );
        assert_eq!(script_position("    at bru-script:7\n"), Some((7, None)));
        assert_eq!(script_position("    at foo (plv-api:3:1)\n"), None);
    }
}
