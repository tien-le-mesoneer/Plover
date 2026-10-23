//! Launch-code exchange, the UI's ready signal, and the event WebSocket.

use axum::Json;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::Response;

use super::dto::{ClientReadyBody, ExchangeBody, SessionTokenDto};
use super::extract::ApiJson;
use crate::error::ApiError;
use crate::security::TOKEN_PROTOCOL_PREFIX;
use crate::session::RedeemError;
use crate::state::AppState;

/// Trades a launch code for the session token. Needs no token (Host/Origin still apply).
pub async fn exchange(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<ExchangeBody>,
) -> Result<Json<SessionTokenDto>, ApiError> {
    let redeemed = state.codes.redeem(&body.code);
    state.client.send_modify(|p| match redeemed {
        Ok(()) => p.codes_exchanged += 1,
        Err(_) => p.codes_rejected += 1,
    });
    match redeemed {
        Ok(()) => Ok(Json(SessionTokenDto {
            token: state.token.to_string(),
        })),
        Err(RedeemError::Invalid) => Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_code",
            "unknown, used or expired launch code",
        )),
        Err(RedeemError::Limited) => Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            "too many invalid launch codes; try again in a minute",
        )),
    }
}

/// Longest page-reported startup error kept (characters); the rest is cut.
const MAX_READY_ERROR_CHARS: usize = 500;

/// The page calls this once its startup finished, with the error if it failed (the
/// desktop smoke test waits for it and prints the error).
pub async fn client_ready(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<ClientReadyBody>,
) -> StatusCode {
    let report = match body.error.filter(|e| !e.trim().is_empty()) {
        None => Ok(()),
        Some(e) => Err(e.chars().take(MAX_READY_ERROR_CHARS).collect()),
    };
    state.client.send_modify(|p| p.ready = Some(report));
    StatusCode::NO_CONTENT
}

pub async fn events(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let protocol = format!("{TOKEN_PROTOCOL_PREFIX}{}", state.token);
    ws.protocols([protocol])
        .on_upgrade(move |socket| forward(socket, state))
}

async fn forward(mut socket: WebSocket, state: AppState) {
    let mut rx = state.events.subscribe();
    loop {
        tokio::select! {
            event = rx.recv() => match event {
                Ok(event) => {
                    let Ok(text) = serde_json::to_string(&event) else { continue };
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                // Missed some events: keep going with the next ones.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => continue,
            },
        }
    }
}
