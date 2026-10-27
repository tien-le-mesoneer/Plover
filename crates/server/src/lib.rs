//! Thin axum shell over `plover-core`: security checks, REST + WebSocket API and
//! the embedded UI. Binds to 127.0.0.1 only.

mod api;
mod assets;
mod error;
mod events;
pub mod native;
mod security;
mod session;
mod state;

use std::future::{Future, IntoFuture};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::routing::get;
use axum::{Json, Router, middleware};
use plover_core::executor::ExecOptions;
use plover_core::secrets::SecretVault;
use plover_core::workspace::Workspace;

pub use api::MAX_INLINE_BODY;
pub use api::dto::typescript_bindings;
pub use error::ApiError;
pub use events::{Events, ServerEvent};
pub use security::AllowList;
pub use session::SessionCodes;
pub use state::AppState;
pub use state::ClientProgress;

#[derive(Default)]
pub struct ServerConfig {
    /// `0` picks a random free port.
    pub port: u16,
    /// Extra origins allowed to call the API (e.g. the Vite dev server).
    pub dev_origins: Vec<String>,
    pub exec: ExecOptions,
    /// Where secret values are kept; `None` keeps them in memory.
    pub secrets: Option<Arc<SecretVault>>,
    /// The desktop shell's native features; `None` in web mode.
    pub native: Option<Arc<dyn native::NativeHost>>,
    /// Where `state.json` lives (open collections, recent list, UI layout); `None`
    /// remembers nothing. Remembered collections are reopened before serving.
    pub state_dir: Option<PathBuf>,
}

pub struct RunningServer {
    pub addr: SocketAddr,
    /// The server's workspace, e.g. to open a collection given on the command line.
    pub workspace: Arc<Workspace>,
    pub token: String,
    /// `http://127.0.0.1:<port>/#token=<token>`, printed by `plv serve` for copying.
    /// URLs the app opens itself use [`RunningServer::launch_url`] instead.
    pub url: String,
    /// Notifies connected pages (collection opened, reload requested).
    pub events: Events,
    /// The page's startup milestones (loaded, launch code traded, ready report).
    pub client: tokio::sync::watch::Receiver<ClientProgress>,
    state: AppState,
}

impl RunningServer {
    /// `http://127.0.0.1:<port>/#code=<single-use code>`: what the app opens itself, so
    /// the token never appears in a URL or browser history.
    pub fn launch_url(&self) -> String {
        self.state.launch_url()
    }
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({ "ok": true })) }),
        )
        .merge(api::routes())
        .fallback(|| async { ApiError::not_found("no such API route") })
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security::require_token,
        ));
    let api = protected.merge(api::public_routes());
    Router::new()
        .nest("/api", api)
        .fallback(assets::serve)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security::check_host_and_origin,
        ))
        .with_state(state)
}

/// Binds `127.0.0.1:<port>`, generates a session token and returns the
/// server future, which runs until `shutdown` completes.
pub async fn start(
    cfg: ServerConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<(RunningServer, impl Future<Output = std::io::Result<()>>)> {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, cfg.port)).await?;
    let addr = listener.local_addr()?;
    let token = security::new_token();
    let base = format!("http://{addr}");
    let url = format!("{base}/#token={token}");
    let mut state = AppState::with_secrets(
        cfg.exec,
        &token,
        AllowList::new(addr.port(), &cfg.dev_origins),
        cfg.secrets
            .unwrap_or_else(|| Arc::new(SecretVault::memory())),
    )
    .with_base_url(&base);
    if let Some(host) = cfg.native {
        state = state.with_native(host);
    }
    if let Some(dir) = &cfg.state_dir {
        state = state
            .with_history(plover_core::run_history::RunHistory::at(dir))
            .with_send_history(plover_core::send_history::SendHistory::at(dir));
    }
    let workspace = state.workspace.clone();
    if let Some(dir) = cfg.state_dir {
        workspace.set_app_state(Arc::new(plover_core::app_state::AppStateStore::load(&dir)));
        // Loading collections reads many files; keep it off the async workers.
        let restoring = workspace.clone();
        tokio::task::spawn_blocking(move || restoring.restore())
            .await
            .map_err(std::io::Error::other)?;
    }
    let events = state.events.clone();
    let client = state.client.subscribe();
    let router_state = state.clone();
    let server = axum::serve(listener, router(router_state))
        .with_graceful_shutdown(shutdown)
        .into_future();
    let running = RunningServer {
        workspace,
        url,
        addr,
        token,
        events,
        client,
        state,
    };
    Ok((running, server))
}
