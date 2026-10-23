//! Capabilities and native host routes (folder picker, open in browser).

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use plover_core::secrets::SecretBackend;

use super::dto::{CapabilitiesDto, PickedFolderDto};
use crate::error::ApiError;
use crate::native::NativeHost;
use crate::state::AppState;

pub async fn capabilities(State(state): State<AppState>) -> Json<CapabilitiesDto> {
    let native = state.native.is_some();
    Json(CapabilitiesDto {
        desktop: native,
        native_picker: native,
        open_external: native,
        secret_backend: match state.workspace.secret_backend() {
            SecretBackend::Keychain => "keychain",
            SecretBackend::Memory => "memory",
        }
        .to_owned(),
    })
}

fn host(state: &AppState) -> Result<&dyn NativeHost, ApiError> {
    state.native.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "not_supported",
            "only available in the desktop app",
        )
    })
}

/// Shows the native folder picker; it does not open the folder (the UI then calls
/// `POST /collections`).
pub async fn pick_folder(State(state): State<AppState>) -> Result<Json<PickedFolderDto>, ApiError> {
    let path = host(&state)?.pick_folder("Open Collection").await;
    Ok(Json(PickedFolderDto {
        path: path.map(|p| p.to_string_lossy().into_owned()),
    }))
}

/// Opens this session in the default browser, with a single-use launch code in the URL
/// (never the token).
pub async fn open_browser(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    host(&state)?
        .open_external(&state.launch_url())
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, "open_failed", e))?;
    Ok(StatusCode::NO_CONTENT)
}
