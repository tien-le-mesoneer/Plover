//! Launch-code exchange, the UI's ready signal, and the event WebSocket.

use axum::Json;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::Response;

use super::dto::{ExchangeBody, SessionTokenDto};
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
    match state.codes.redeem(&body.code) {
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

/// The page calls this once it has mounted (the desktop smoke test waits for it).
pub async fn client_ready(State(state): State<AppState>) -> StatusCode {
    state.client_ready.send_replace(true);
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
