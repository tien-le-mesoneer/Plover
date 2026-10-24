//! `plv`: run Bruno collections from the command line. The desktop app embeds the
//! same CLI through [`main_with_args`].

mod print;

use std::ffi::OsStr;
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use plover_core::collection::Collection;
use plover_core::data_file::{self, DataFile, DataFormat};
use plover_core::executor::{ExecOptions, Session};
use plover_core::masking::secret_strings;
use plover_core::report::{self, ReportFilter};
use plover_core::run_history::{HistoryError, RunHistory};
use plover_core::run_record::{RunEnd, RunHeader, RunId, RunRecord, RunRecorder, RunSource};
use plover_core::runner::{self, Iterations, Persist, RunEvent, RunOptions, RunVars};
use plover_core::script::ScriptLimits;
use plover_core::vars::{ValueMap, VarMap};
use plover_core::workspace::CollectionId;

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
    /// List, show or clear saved runs.
    Runs(RunsArgs),
}

#[derive(clap::Args)]
#[command(args_conflicts_with_subcommands = true)]
struct RunsArgs {
    #[command(subcommand)]
    action: Option<RunsAction>,
    /// Collection whose runs to list (default: the current directory).
    dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum RunsAction {
    /// Print a saved run and write its reports, without sending anything.
    Show(ShowArgs),
    /// Delete a collection's saved runs.
    Clear(ClearArgs),
}

#[derive(clap::Args)]
struct ShowArgs {
    /// The run ID, as `plv runs` lists it.
    id: String,
    #[command(flatten)]
    reports: ReportArgs,
}

#[derive(clap::Args)]
struct ClearArgs {
    /// The collection (default: the current directory).
    dir: Option<PathBuf>,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

/// Report files and Bruno's `--reporter-skip-*` flags.
#[derive(clap::Args)]
struct ReportArgs {
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
}

impl ReportArgs {
    /// Fails when a report's directory does not exist, before anything runs.
    fn check_dirs(&self) -> Result<(), String> {
        for report in [
            &self.reporter_junit,
            &self.reporter_json,
            &self.reporter_html,
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
        Ok(())
    }

    fn write(&self, record: &RunRecord) -> Result<(), String> {
        write_reports(
            record,
            &self.skip.filter(),
            self.reporter_junit.as_deref(),
            self.reporter_json.as_deref(),
            self.reporter_html.as_deref(),
        )
    }
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
    #[command(flatten)]
    reports: ReportArgs,
    /// Save this run to the run history even when `CI` is set.
    #[arg(long, conflicts_with = "no_history")]
    history: bool,
    /// Do not save this run to the run history.
    #[arg(long)]
    no_history: bool,
    /// Wait this many milliseconds after each request except the last (Bruno `--delay`),
    /// e.g. to stay under a server's rate limit.
    #[arg(long, value_name = "MS")]
    delay: Option<u64>,
    /// Run once per row of this CSV file (`{{column}}` reads the row).
    #[arg(
        long = "csv-file-path",
        value_name = "FILE",
        conflicts_with = "json_file_path"
    )]
    csv_file_path: Option<PathBuf>,
    /// Run once per element of this JSON array of objects.
    #[arg(long = "json-file-path", value_name = "FILE")]
    json_file_path: Option<PathBuf>,
    /// Run this many times (a data file sets the count to its rows instead).
    #[arg(long = "iteration-count", value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    iteration_count: Option<u32>,
    /// Accepted for `bru run` compatibility; iterations still run one after another.
    #[arg(long)]
    parallel: bool,
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
            Command::Runs(args) => runs(args),
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
    args.reports.check_dirs()?;
    let data = load_data(&args)?;
    if args.parallel {
        eprintln!("--parallel is not supported yet; iterations run one after another");
    }
    let iterations = match (&data, args.iteration_count) {
        (Some((name, file)), count) => {
            if count.is_some() {
                eprintln!(
                    "--iteration-count ignored: {name} has {} rows",
                    file.rows.len()
                );
            }
            Iterations::from_rows(file.rows.clone())
        }
        (None, Some(count)) => Iterations::repeat(count as usize),
        (None, None) => Iterations::default(),
    };
    let data_secrets = data
        .as_ref()
        .map(|(_, f)| data_file::credential_values(&f.rows))
        .unwrap_or_default();
    let show_iterations = data.is_some() || iterations.count > 1;
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
        iterations,
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
    let mut start_secrets = secrets(&state);
    start_secrets.extend(data_secrets);
    let mut recorder = RunRecorder::new(
        &collection.root,
        run_header(&collection, &args.path, opts.env.clone()),
        start_secrets,
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
            recorder.event(event);
            match event {
                RunEvent::IterationStarted { index, total, .. } if show_iterations => {
                    let data = recorder
                        .masked_iteration(*index)
                        .map(|i| i.data)
                        .unwrap_or_default();
                    printer.iteration(*index, *total, &data);
                }
                _ => printer.event(event),
            }
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    print::summary(&report);
    let record = recorder.finish(RunEnd::Finished {
        report,
        end_secrets: secrets(&state),
    });
    args.reports.write(&record)?;
    if history_enabled(
        args.history,
        args.no_history,
        std::env::var_os("CI").as_deref(),
    ) {
        save_run(&record);
    }
    Ok(if report.failed > 0 {
        ExitCode::from(EXIT_FAILED)
    } else {
        ExitCode::SUCCESS
    })
}

/// The data file named by `--csv-file-path` / `--json-file-path` (relative to the current
/// directory, like `bru`), parsed; every way of getting no rows is an error.
fn load_data(args: &RunArgs) -> Result<Option<(String, DataFile)>, String> {
    let (path, format) = match (&args.csv_file_path, &args.json_file_path) {
        (Some(p), _) => (p, DataFormat::Csv),
        (None, Some(p)) => (p, DataFormat::Json),
        (None, None) => return Ok(None),
    };
    let name = path.display().to_string();
    let size = std::fs::metadata(path)
        .map_err(|e| format!("{name}: cannot read the data file: {e}"))?
        .len();
    if size > data_file::MAX_DATA_FILE as u64 {
        return Err(data_file::DataFileError::TooLarge {
            size: usize::try_from(size).unwrap_or(usize::MAX),
        }
        .with_name(&name));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("{name}: cannot read the data file: {e}"))?;
    let file = data_file::parse_file(format, &text).map_err(|e| e.with_name(&name))?;
    Ok(Some((name, file)))
}

/// `true` unless `CI` is set to a truthy value; `--history` and `--no-history` win.
fn history_enabled(force: bool, disable: bool, ci: Option<&OsStr>) -> bool {
    if force || disable {
        return force;
    }
    let ci = ci.map(|v| v.to_string_lossy().trim().to_ascii_lowercase());
    matches!(ci.as_deref(), None | Some("" | "0" | "false"))
}

/// Saves the run; a failure only warns, so the run's result and exit code stay.
fn save_run(record: &RunRecord) {
    let Some(dir) = plover_core::app_state::default_dir() else {
        eprintln!("warning: no state directory, the run is not saved to history");
        return;
    };
    match RunHistory::at(&dir).save(record) {
        Ok(()) => eprintln!("Saved run {}", record.header.id),
        Err(e) => eprintln!("warning: cannot save run history: {e}"),
    }
}

fn history() -> Result<RunHistory, String> {
    plover_core::app_state::default_dir()
        .map(|dir| RunHistory::at(&dir))
        .ok_or_else(|| "no state directory (set PLOVER_STATE_DIR)".to_owned())
}

/// The collection at `dir` (default `.`): its history key and name.
fn collection_of(dir: Option<&Path>) -> Result<(CollectionId, String), String> {
    let root = Collection::find_root(dir.unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    let id = CollectionId::for_root(&root).map_err(|e| e.to_string())?;
    let name = Collection::load(&root)
        .map(|c| c.config.name)
        .unwrap_or_else(|_| {
            root.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    Ok((id, name))
}

fn runs(args: RunsArgs) -> Result<ExitCode, String> {
    match args.action {
        None => {
            let (id, name) = collection_of(args.dir.as_deref())?;
            let metas = history()?.list(&id);
            if metas.is_empty() {
                println!("No runs saved for {name}");
            } else {
                print::run_list(&metas);
            }
            Ok(ExitCode::SUCCESS)
        }
        Some(RunsAction::Show(show)) => {
            let Some(id) = RunId::parse(&show.id) else {
                eprintln!("error: invalid run ID `{}`", show.id);
                return Ok(ExitCode::from(EXIT_SETUP));
            };
            show.reports.check_dirs()?;
            let record = match history()?.find(&id) {
                Ok(record) => record,
                Err(e @ (HistoryError::NotFound(_) | HistoryError::Unreadable { .. })) => {
                    eprintln!("error: {e}");
                    return Ok(ExitCode::from(EXIT_SETUP));
                }
                Err(e) => return Err(e.to_string()),
            };
            print::run_record(&record);
            show.reports.write(&record)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(RunsAction::Clear(clear)) => {
            let (id, name) = collection_of(clear.dir.as_deref())?;
            let history = history()?;
            let count = history.list(&id).len();
            if !clear.yes {
                let stdin = std::io::stdin();
                if !stdin.is_terminal() {
                    eprintln!("error: not a terminal; pass --yes to delete the runs of {name}");
                    return Ok(ExitCode::from(EXIT_SETUP));
                }
                eprint!("Delete {count} runs of {name}? [y/N] ");
                let mut answer = String::new();
                stdin.read_line(&mut answer).map_err(|e| e.to_string())?;
                if !matches!(answer.trim(), "y" | "Y" | "yes") {
                    eprintln!("Nothing deleted");
                    return Ok(ExitCode::SUCCESS);
                }
            }
            let deleted = history.clear(&id).map_err(|e| e.to_string())?;
            println!("Deleted {deleted} runs of {name}");
            Ok(ExitCode::SUCCESS)
        }
    }
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
