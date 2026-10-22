//! Thin axum shell over `brs-core`: security checks, REST + WebSocket API and
//! the embedded UI. Binds to 127.0.0.1 only.

mod api;
mod assets;
mod error;
mod security;
mod state;

use std::future::{Future, IntoFuture};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::routing::get;
use axum::{Json, Router, middleware};
use brs_core::executor::ExecOptions;
use brs_core::workspace::Workspace;

pub use api::dto::typescript_bindings;
pub use error::ApiError;
pub use security::AllowList;
pub use state::AppState;

pub struct ServerConfig {
    /// `0` picks a random free port.
    pub port: u16,
    /// Extra origins allowed to call the API (e.g. the Vite dev server).
    pub dev_origins: Vec<String>,
    pub exec: ExecOptions,
}

pub struct RunningServer {
    pub addr: SocketAddr,
    /// The server's workspace, e.g. to open a collection given on the command line.
    pub workspace: Arc<Workspace>,
    pub token: String,
    /// `http://127.0.0.1:<port>/#token=<token>`; the fragment never reaches the server.
    pub url: String,
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
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
    let workspace = Arc::new(Workspace::new(cfg.exec));
    let state = AppState {
        workspace: workspace.clone(),
        token: Arc::from(token.as_str()),
        allowed: Arc::new(AllowList::new(addr.port(), &cfg.dev_origins)),
    };
    let server = axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .into_future();
    let running = RunningServer {
        workspace,
        url: format!("http://{addr}/#token={token}"),
        addr,
        token,
    };
    Ok((running, server))
}
