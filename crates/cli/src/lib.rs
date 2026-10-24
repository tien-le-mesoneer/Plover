//! `plv`: run Bruno collections from the command line. The desktop app embeds the
//! same CLI through [`main_with_args`].

mod print;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use plover_core::collection::Collection;
use plover_core::executor::{ExecOptions, Session};
use plover_core::masking::secret_strings;
use plover_core::report::{self, ReportFilter};
use plover_core::run_record::{RunEnd, RunHeader, RunId, RunRecord, RunRecorder, RunSource};
use plover_core::runner::{self, Persist, RunOptions, RunVars};
use plover_core::script::ScriptLimits;
use plover_core::vars::{ValueMap, VarMap};

#[derive(Parser)]
#[command(name = "plv", version, about = "Run Bruno-compatible API collections")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a collection, a folder, or a single .bru request.
    Run(RunArgs),
    /// Start the local web UI on 127.0.0.1.
    Serve(ServeArgs),
}

#[derive(clap::Args)]
struct ServeArgs {
    /// Collection to open at startup (its root, a folder, or a .bru file).
    path: Option<PathBuf>,
    /// Port to listen on; 0 picks a random free port.
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Print the URL without opening a browser.
    #[arg(long)]
    no_open: bool,
    /// Also allow this origin to call the API (repeatable), e.g. the Vite dev server.
    #[arg(long = "dev-origin", value_name = "URL")]
    dev_origins: Vec<String>,
}

#[derive(clap::Args)]
struct RunArgs {
    /// Collection directory, folder, or .bru file.
    path: PathBuf,
    /// Environment name from the collection's `environments/` folder.
    #[arg(long)]
    env: Option<String>,
    /// Override or supply an environment variable (repeatable), e.g. `--env-var token=abc`.
    #[arg(long = "env-var", value_name = "KEY=VALUE", value_parser = parse_key_value)]
    env_vars: Vec<(String, String)>,
    /// Do not verify TLS certificates.
    #[arg(long)]
    insecure: bool,
    /// Request timeout in milliseconds (default: none).
    #[arg(long, value_name = "N")]
    timeout_ms: Option<u64>,
    /// Do not follow HTTP redirects.
    #[arg(long)]
    no_follow_redirects: bool,
    /// Write a JUnit XML report to this file.
    #[arg(long, value_name = "FILE")]
    reporter_junit: Option<PathBuf>,
    /// Write a JSON report to this file.
    #[arg(long, value_name = "FILE")]
    reporter_json: Option<PathBuf>,
    /// Write a self-contained HTML report to this file.
    #[arg(long, value_name = "FILE")]
    reporter_html: Option<PathBuf>,
    #[command(flatten)]
    skip: SkipArgs,
    /// Wait this many milliseconds after each request except the last (Bruno `--delay`),
    /// e.g. to stay under a server's rate limit.
    #[arg(long, value_name = "MS")]
    delay: Option<u64>,
    /// Accepted for `bru run` compatibility: Plover always runs folders recursively.
    #[arg(short = 'r', long = "recursive")]
    _recursive: bool,
    /// Accepted for `bru run` compatibility: Plover runs scripts like Bruno's `safe`
    /// sandbox; `developer` (Node modules, filesystem) is not supported.
    #[arg(long, value_name = "SANDBOX", value_parser = ["safe"])]
    _sandbox: Option<String>,
    /// Time limit for each script phase, in milliseconds.
    #[arg(long, value_name = "N", default_value_t = 30_000)]
    script_timeout_ms: u64,
    /// Memory limit for scripts, in MiB.
    #[arg(long, value_name = "N", default_value_t = 128)]
    script_memory_mb: usize,
    /// Also print what scripts log with console.*.
    #[arg(long)]
    verbose: bool,
    /// Read the environment's secret values from the OS keychain (as stored by
    /// `plv serve` or the desktop app); `--env-var` still wins. Needs `--env`.
    #[arg(long, requires = "env")]
    use_keychain: bool,
}

/// Bruno's `--reporter-skip-*` flags: leave parts of the JSON and HTML reports out.
#[derive(clap::Args, Clone, Default)]
struct SkipArgs {
    /// Omit all request and response headers from the reports.
    #[arg(long)]
    reporter_skip_all_headers: bool,
    /// Omit these headers from the reports (case-insensitive, repeatable).
    #[arg(long, value_name = "NAME", num_args = 1..)]
    reporter_skip_headers: Vec<String>,
    /// Omit request bodies from the reports.
    #[arg(long)]
    reporter_skip_request_body: bool,
    /// Omit response bodies from the reports.
    #[arg(long)]
    reporter_skip_response_body: bool,
    /// Omit request and response bodies from the reports.
    #[arg(long)]
    reporter_skip_body: bool,
}

impl SkipArgs {
    fn filter(&self) -> ReportFilter {
        ReportFilter {
            skip_all_headers: self.reporter_skip_all_headers,
            skip_headers: self.reporter_skip_headers.clone(),
            skip_request_body: self.reporter_skip_request_body || self.reporter_skip_body,
            skip_response_body: self.reporter_skip_response_body || self.reporter_skip_body,
        }
    }
}

const EXIT_FAILED: u8 = 1;
const EXIT_SETUP: u8 = 2;

fn parse_key_value(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .filter(|(k, _)| !k.is_empty())
        .ok_or_else(|| format!("expected KEY=VALUE, got `{s}`"))
}

/// Runs the CLI with `args` (the first is the program name) and returns its exit code.
pub fn main_with_args<I, T>(args: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() {
                ExitCode::from(EXIT_SETUP)
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .try_init();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("error: cannot start the async runtime: {e}");
            return ExitCode::from(EXIT_SETUP);
        }
    };
    let result = runtime.block_on(async {
        match cli.command {
            Command::Run(args) => run(args).await,
            Command::Serve(args) => serve(args).await,
        }
    });
    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(EXIT_SETUP)
        }
    }
}

async fn run(args: RunArgs) -> Result<ExitCode, String> {
    if !args.path.exists() {
        return Err(format!("{} does not exist", args.path.display()));
    }
    for report in [
        &args.reporter_junit,
        &args.reporter_json,
        &args.reporter_html,
    ]
    .into_iter()
    .flatten()
    {
        let dir = report.parent().filter(|d| !d.as_os_str().is_empty());
        if dir.is_some_and(|d| !d.is_dir()) {
            return Err(format!(
                "output directory {} does not exist",
                dir.unwrap_or(Path::new(".")).display()
            ));
        }
    }
    let root = Collection::find_root(&args.path).map_err(|e| e.to_string())?;
    let collection = Collection::load(&root).map_err(|e| e.to_string())?;
    let keychain = match (&args.env, args.use_keychain) {
        (Some(env), true) => plover_core::secrets::env_secrets(
            &plover_core::secrets::default_store(),
            &collection,
            env,
        ),
        _ => VarMap::new(),
    };
    let opts = RunOptions {
        env: args.env,
        exec: ExecOptions {
            timeout: args.timeout_ms.map(Duration::from_millis),
            follow_redirects: !args.no_follow_redirects,
            verify_tls: !args.insecure,
            ..ExecOptions::default()
        },
        scripts: ScriptLimits {
            timeout: Duration::from_millis(args.script_timeout_ms),
            memory_bytes: args.script_memory_mb.saturating_mul(1024 * 1024),
            ..ScriptLimits::default()
        },
        // Bruno CLI writes script changes to the environment file and collection.bru.
        persist: Persist::All,
        delay: args.delay.map(Duration::from_millis),
        ..RunOptions::default()
    }
    .with_env_vars(&keychain)
    .with_env_vars(&args.env_vars.into_iter().collect::<VarMap>());
    let environment = match &opts.env {
        Some(name) => Some(
            collection
                .environment(name)
                .ok_or_else(|| format!("environment `{name}` not found"))?,
        ),
        None => None,
    };
    let mut state = RunVars::start(
        &collection,
        environment,
        &opts.env_overrides,
        ValueMap::new(),
    );
    let secrets =
        |state: &RunVars| secret_strings(&runner::secret_values(environment, &state.environment));
    let mut recorder = RunRecorder::new(
        &collection.root,
        run_header(&collection, &args.path, opts.env.clone()),
        secrets(&state),
    );
    let session = Session::new(opts.exec.clone()).map_err(|e| e.to_string())?;
    let mut printer = print::Printer::new(&collection.root, args.verbose);
    let report = runner::run_with_state(
        &collection,
        &args.path,
        &opts,
        &session,
        &mut state,
        &mut |event| {
            printer.event(event);
            recorder.event(event);
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    print::summary(&report);
    let record = recorder.finish(RunEnd::Finished {
        report,
        end_secrets: secrets(&state),
    });
    write_reports(
        &record,
        &args.skip.filter(),
        args.reporter_junit.as_deref(),
        args.reporter_json.as_deref(),
        args.reporter_html.as_deref(),
    )?;
    Ok(if report.failed > 0 {
        ExitCode::from(EXIT_FAILED)
    } else {
        ExitCode::SUCCESS
    })
}

/// Which run this is: the collection, what ran, and when it started.
fn run_header(collection: &Collection, target: &Path, env: Option<String>) -> RunHeader {
    let root = collection
        .root
        .canonicalize()
        .unwrap_or_else(|_| collection.root.clone());
    let absolute = std::path::absolute(target)
        .ok()
        .and_then(|t| t.canonicalize().ok())
        .unwrap_or_else(|| target.to_path_buf());
    let relative: Vec<String> = absolute
        .strip_prefix(&root)
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let now = std::time::SystemTime::now();
    RunHeader {
        id: RunId::new(now),
        source: RunSource::Plv,
        collection_root: root.to_string_lossy().into_owned(),
        collection_name: collection.config.name.clone(),
        target: if relative.is_empty() {
            ".".to_owned()
        } else {
            relative.join("/")
        },
        env,
        started_at_ms: plover_core::time::unix_ms(now),
    }
}

/// Renders the reports asked for from the masked run.
fn write_reports(
    record: &RunRecord,
    filter: &ReportFilter,
    junit: Option<&Path>,
    json: Option<&Path>,
    html: Option<&Path>,
) -> Result<(), String> {
    let write = |path: &Path, text: String| {
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    };
    if let Some(path) = junit {
        write(path, report::junit::render(record))?;
        println!("Wrote junit results to {}", path.display());
    }
    if let Some(path) = json {
        let text = serde_json::to_string_pretty(&report::json::render(record, filter))
            .map_err(|e| e.to_string())?;
        write(path, text)?;
        println!("Wrote json results to {}", path.display());
    }
    if let Some(path) = html {
        write(path, report::html::render(record, filter))?;
        println!("Wrote html results to {}", path.display());
    }
    Ok(())
}

async fn serve(args: ServeArgs) -> Result<ExitCode, String> {
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let (running, server) = plover_server::start(
        plover_server::ServerConfig {
            port: args.port,
            dev_origins: args.dev_origins,
            exec: ExecOptions::default(),
            secrets: Some(plover_core::secrets::default_store()),
            native: None,
            // Collections opened here come back on the next `plv serve`.
            state_dir: plover_core::app_state::default_dir(),
        },
        shutdown,
    )
    .await
    .map_err(|e| format!("cannot start server: {e}"))?;
    if let Some(path) = &args.path {
        running
            .workspace
            .open(path)
            .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    }
    // The printed URL carries the token so it can be copied; the browser we open
    // ourselves gets a single-use launch code instead, keeping the token out of history.
    println!("plv listening on {}", running.url);
    if !args.no_open
        && let Err(e) = webbrowser::open(&running.launch_url())
    {
        eprintln!("could not open a browser ({e}); open the URL above");
    }
    server.await.map_err(|e| format!("server error: {e}"))?;
    Ok(ExitCode::SUCCESS)
}
