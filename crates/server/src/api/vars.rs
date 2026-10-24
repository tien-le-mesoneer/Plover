//! "Save as runtime variable" from the response viewer's value menu.

use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use ts_rs::TS;

use super::extract::{ApiJson, ApiPath};
use super::open_collection;
use crate::error::ApiError;
use crate::state::AppState;

/// `PUT /collections/{id}/runtime-vars/{name}`.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeVarBody {
    #[ts(type = "unknown")]
    pub value: serde_json::Value,
}

/// A name `{{name}}` and `bru.getVar(name)` can use: a letter or `_`, then word
/// characters, `.` or `-`.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        && name.len() <= 256
}

/// Sets a runtime variable like `bru.setVar`: the next send or run sees it.
pub async fn set_runtime_var(
    State(state): State<AppState>,
    ApiPath((id, name)): ApiPath<(String, String)>,
    ApiJson(body): ApiJson<RuntimeVarBody>,
) -> Result<StatusCode, ApiError> {
    let open = open_collection(&state, &id)?;
    if !valid_name(&name) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_name",
            format!("`{name}` is not a variable name"),
        ));
    }
    open.set_runtime_var(&name, body.value);
    Ok(StatusCode::NO_CONTENT)
}
