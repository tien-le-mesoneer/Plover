//! Request checks for a localhost server: Host and Origin allow-lists
//! (DNS rebinding and cross-site requests) and the per-process session token.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use subtle::ConstantTimeEq;

use crate::error::ApiError;
use crate::state::AppState;

/// WebSocket clients send the token as this subprotocol prefix, because
/// browsers cannot set headers on a WebSocket handshake.
pub const TOKEN_PROTOCOL_PREFIX: &str = "brs.token.";

#[derive(Debug, Clone)]
pub struct AllowList {
    origins: Vec<String>,
    hosts: Vec<String>,
}

impl AllowList {
    pub fn new(port: u16, dev_origins: &[String]) -> AllowList {
        let mut origins = vec![
            format!("http://127.0.0.1:{port}"),
            format!("http://localhost:{port}"),
        ];
        let mut hosts = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")];
        for origin in dev_origins {
            let origin = origin.trim_end_matches('/').to_ascii_lowercase();
            if let Some((_, host)) = origin.split_once("://") {
                hosts.push(host.to_owned());
            }
            origins.push(origin);
        }
        AllowList { origins, hosts }
    }

    fn host_allowed(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        self.hosts.contains(&host)
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        let origin = origin.to_ascii_lowercase();
        self.origins.contains(&origin)
    }
}

/// 32 random bytes from the OS-seeded thread RNG, hex-encoded.
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn check_host_and_origin(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let headers = req.headers();
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned)
        .or_else(|| req.uri().authority().map(|a| a.to_string()));
    if !host.is_some_and(|h| state.allowed.host_allowed(&h)) {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden_host",
            "unexpected Host header",
        )
        .into_response();
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let ok = origin
            .to_str()
            .is_ok_and(|o| state.allowed.origin_allowed(o));
        if !ok {
            return ApiError::new(
                StatusCode::FORBIDDEN,
                "forbidden_origin",
                "unexpected Origin",
            )
            .into_response();
        }
    }
    next.run(req).await
}

pub async fn require_token(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if presented_token(req.headers()).is_some_and(|t| token_eq(t, &state.token)) {
        return next.run(req).await;
    }
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "missing or invalid session token",
    )
    .into_response()
}

fn presented_token(headers: &HeaderMap) -> Option<&str> {
    if let Some(bearer) = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
    {
        return Some(bearer);
    }
    headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|h| h.to_str().ok())?
        .split(',')
        .map(str::trim)
        .find_map(|p| p.strip_prefix(TOKEN_PROTOCOL_PREFIX))
}

fn token_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}
