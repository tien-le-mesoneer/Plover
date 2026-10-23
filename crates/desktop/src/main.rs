//! Plover desktop: starts the same local server as `plv serve` on 127.0.0.1 with a
//! random port and token, and shows it in a native window. Native features (folder
//! picker, open in browser) reach the UI through the server API (decision P4-1).
//! Invoked as `plv` (or with `run`/`serve`) the same binary is the CLI (decision P4-5).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli_link;
mod host;
mod lifecycle;
mod menu;

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;
use std::sync::{Arc, Mutex, PoisonError};

use plover_server::{RunningServer, ServerConfig, ServerEvent};
use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

use crate::host::DesktopHost;
use crate::lifecycle::{SHUTDOWN_DEADLINE, ServerRuntime, Shutdown};

const MAIN_WINDOW: &str = "main";

/// The running server, kept in Tauri state.
struct Desktop {
    running: RunningServer,
    /// Taken on Quit.
    server: Mutex<Option<ServerRuntime>>,
}

fn smoke_mode() -> bool {
    std::env::var_os("PLOVER_DESKTOP_SMOKE").is_some()
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    if cli_link::is_cli(&args, std::env::var_os("PLOVER_AS_CLI").is_some()) {
        attach_parent_console();
        // Report the program as `plv` in help and errors, whatever the file is called.
        let cli_args = std::iter::once("plv".into()).chain(args.into_iter().skip(1));
        return plover_cli::main_with_args(cli_args);
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let mut builder = tauri::Builder::default();
    // The smoke test must exercise its own window, never hand off to a running app.
    if !smoke_mode() {
        // Registered first: a second launch focuses this one instead.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            let argv: Vec<OsString> = argv.into_iter().map(OsString::from).collect();
            on_second_instance(app, &argv, Path::new(&cwd));
        }));
    }
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            setup(app.handle(), &args);
            Ok(())
        })
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .build(tauri::generate_context!());
    let app = match app {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Plover could not start: {e}");
            return ExitCode::FAILURE;
        }
    };
    app.run(|app, event| {
        if let RunEvent::ExitRequested { .. } = event {
            stop_server(app);
        }
    });
    ExitCode::SUCCESS
}

fn setup(app: &AppHandle<Wry>, args: &[OsString]) {
    let cfg = ServerConfig {
        secrets: Some(plover_core::secrets::default_store()),
        native: Some(Arc::new(DesktopHost { app: app.clone() })),
        ..ServerConfig::default()
    };
    let (server, running) = match lifecycle::launch(cfg) {
        Ok(started) => started,
        Err(message) => {
            let app = app.clone();
            app.dialog()
                .message(message)
                .title("Plover")
                .kind(MessageDialogKind::Error)
                .show(move |_| app.exit(1));
            return;
        }
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Some(path) = lifecycle::path_from_args(args, &cwd)
        && let Err(e) = running.workspace.open(&path)
    {
        tracing::warn!(path = %path.display(), error = %e, "could not open the collection given on the command line");
    }
    // The window gets a single-use launch code, never the token.
    let launch_url = running.launch_url();
    if smoke_mode() {
        smoke_test(app, &server, &running);
    }
    app.manage(Desktop {
        running,
        server: Mutex::new(Some(server)),
    });

    if let Err(e) = menu::build(app).and_then(|m| app.set_menu(m)) {
        tracing::warn!(error = %e, "could not build the menu");
    }
    let url: Url = match launch_url.parse() {
        Ok(url) => url,
        Err(e) => {
            show_error(app, &format!("Plover could not open its window: {e}"));
            return;
        }
    };
    let window = WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::External(url))
        .title("Plover")
        .inner_size(1280.0, 820.0)
        .min_inner_size(800.0, 500.0)
        .build();
    if let Err(e) = window {
        show_error(app, &format!("Plover could not open its window: {e}"));
    }
}

fn desktop(app: &AppHandle<Wry>) -> Option<tauri::State<'_, Desktop>> {
    app.try_state::<Desktop>()
}

fn show_error(app: &AppHandle<Wry>, message: &str) {
    app.dialog()
        .message(message)
        .title("Plover")
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}

/// Opens a collection natively and tells the page, which adds it without reloading
/// (open tabs and unsaved drafts stay).
fn open_collection(app: &AppHandle<Wry>, path: &Path) {
    let Some(state) = desktop(app) else { return };
    match state.running.workspace.open(path) {
        Ok(id) => state
            .running
            .events
            .send(ServerEvent::CollectionOpened { id: id.0 }),
        Err(e) => show_error(app, &format!("Could not open {}: {e}", path.display())),
    }
}

/// Reload Window: the page reloads itself, asking first if edits are unsaved. Without a
/// connected page there is nothing to lose, so the window reloads directly.
fn reload_window(app: &AppHandle<Wry>) {
    let Some(state) = desktop(app) else { return };
    if state.running.events.receivers() > 0 {
        state.running.events.send(ServerEvent::ReloadRequested);
    } else if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.reload();
    }
}

fn on_menu(app: &AppHandle<Wry>, id: &str) {
    match id {
        menu::OPEN_COLLECTION => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(path) = host::pick_folder(&app, "Open Collection").await {
                    open_collection(&app, &path);
                }
            });
        }
        menu::OPEN_IN_BROWSER => {
            if let Some(state) = desktop(app)
                && let Err(e) = app
                    .opener()
                    .open_url(state.running.launch_url(), None::<&str>)
            {
                show_error(app, &format!("Could not open a browser: {e}"));
            }
        }
        menu::RELOAD => reload_window(app),
        menu::INSTALL_CLI => install_cli(app),
        menu::QUIT => app.exit(0),
        _ => {}
    }
}

fn on_second_instance(app: &AppHandle<Wry>, argv: &[OsString], cwd: &Path) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    if let Some(path) = lifecycle::path_from_args(argv, cwd) {
        open_collection(app, &path);
    }
}

/// Stops the server: waits at most `SHUTDOWN_DEADLINE`, then cancels what is left
/// (connections, in-flight runs) by shutting its runtime down.
fn stop_server(app: &AppHandle<Wry>) {
    let Some(state) = desktop(app) else { return };
    let server = state
        .server
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(server) = server
        && server.stop(SHUTDOWN_DEADLINE) == Shutdown::Cancelled
    {
        tracing::info!("server still busy after {SHUTDOWN_DEADLINE:?}; cancelled it");
    }
}

/// `PLOVER_DESKTOP_SMOKE=1`: exits 0 once the page has loaded in the window, traded its
/// launch code for the token and reported ready (`POST /api/client-ready`), and the
/// server answers an authorised request with the window's Host; exits 1 after 20 s.
fn smoke_test(app: &AppHandle<Wry>, server: &ServerRuntime, running: &RunningServer) {
    let (addr, token) = (running.addr, running.token.clone());
    let mut ready = running.client_ready.clone();
    let app = app.clone();
    server.spawn(async move {
        let check = async {
            if ready.wait_for(|r| *r).await.is_err() {
                return false;
            }
            health(addr, &token).await.unwrap_or(false)
        };
        let ok = tokio::time::timeout(std::time::Duration::from_secs(20), check)
            .await
            .unwrap_or(false);
        println!("plover smoke: {}", if ok { "ok" } else { "failed" });
        app.exit(if ok { 0 } else { 1 });
    });
}

/// Release builds on Windows use the GUI subsystem, which has no console: attach to
/// the parent's (the terminal that ran `plv`) so CLI output is visible.
#[cfg(windows)]
fn attach_parent_console() {
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    // SAFETY: AttachConsole has no preconditions; failure (no parent console) is harmless.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}

/// "Install Command Line Tool…": links `plv` to this executable (macOS/Linux).
fn install_cli(app: &AppHandle<Wry>) {
    let target = std::env::current_exe()
        .map(|exe| cli_link::link_target(&exe, std::env::var_os("APPIMAGE").as_deref()));
    let exe = match target {
        Ok(Ok(exe)) => exe,
        Ok(Err(cli_link::LinkTargetError::MoveToApplications)) => {
            app.dialog()
                .message("Move Plover to the Applications folder first, open it from there, then install the command line tool.")
                .title("Plover")
                .kind(MessageDialogKind::Warning)
                .show(|_| {});
            return;
        }
        Err(e) => {
            show_error(
                app,
                &format!("Could not install the command line tool: {e}"),
            );
            return;
        }
    };
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let result = cli_link::install_link(
        &exe,
        &cli_link::default_candidates(&home),
        std::env::var_os("PATH").as_deref(),
    );
    let (kind, message) = match result {
        Ok(installed) if installed.on_path => (
            MessageDialogKind::Info,
            format!(
                "Installed {}. Run `plv --help` in a new terminal.",
                installed.path.display()
            ),
        ),
        Ok(installed) => (
            MessageDialogKind::Warning,
            format!(
                "Installed {}, but {} is not on your PATH. Add it to your shell profile to run `plv`.",
                installed.path.display(),
                installed
                    .path
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            ),
        ),
        Err(cli_link::LinkError::ForeignFile(path)) => (
            MessageDialogKind::Error,
            format!(
                "{} already exists and is not Plover's; it was left unchanged.",
                path.display()
            ),
        ),
        Err(cli_link::LinkError::Io(e)) => (
            MessageDialogKind::Error,
            format!("Could not install the command line tool: {e}"),
        ),
    };
    app.dialog()
        .message(message)
        .title("Plover")
        .kind(kind)
        .show(|_| {});
}

async fn health(addr: std::net::SocketAddr, token: &str) -> std::io::Result<bool> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    let request = format!(
        "GET /api/health HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await?;
    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    Ok(response.starts_with("HTTP/1.1 200"))
}
