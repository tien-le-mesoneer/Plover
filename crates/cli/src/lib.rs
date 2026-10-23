//! `plv`: run Bruno collections from the command line. The desktop app embeds the
//! same CLI through [`main_with_args`].

mod print;
mod report;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use plover_core::collection::Collection;
use plover_core::executor::ExecOptions;
use plover_core::runner::{self, Persist, RunOptions};
use plover_core::script::ScriptLimits;
use plover_core::vars::VarMap;

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
    for report in [&args.reporter_junit, &args.reporter_json]
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
        ..RunOptions::default()
    }
    .with_env_vars(&keychain)
    .with_env_vars(&args.env_vars.into_iter().collect::<VarMap>());
    let mut printer = print::Printer::new(&collection.root, args.verbose);
    let mut collector = report::Collector::new(&collection.root);
    let report = runner::run(&collection, &args.path, &opts, &mut |event| {
        printer.event(event);
        collector.event(event);
    })
    .await
    .map_err(|e| e.to_string())?;
    print::summary(&report);
    if let Some(path) = &args.reporter_junit {
        std::fs::write(path, report::junit::render(&collector.records))
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        println!("Wrote junit results to {}", path.display());
    }
    if let Some(path) = &args.reporter_json {
        let json = serde_json::to_string_pretty(&report::json::render(&collector.records))
            .map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        println!("Wrote json results to {}", path.display());
    }
    Ok(if report.failed > 0 {
        ExitCode::from(EXIT_FAILED)
    } else {
        ExitCode::SUCCESS
    })
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
