//! "Save as runtime variable" from the response viewer's value menu, the runtime-variable
//! list (view, remove, clear), and the variable hover: where each `{{name}}` would come
//! from.

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use plover_core::credentials::is_credential_field;
use plover_core::request::HttpRequest;
use plover_core::runner::{SendInput, preview_stack};
use plover_core::vars::{VarSource, value_text};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::dto::HttpRequestDto;
use super::extract::{ApiJson, ApiPath, ApiQuery};
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

/// Removes one runtime variable; `204` also when it was not set.
pub async fn delete_runtime_var(
    State(state): State<AppState>,
    ApiPath((id, name)): ApiPath<(String, String)>,
) -> Result<StatusCode, ApiError> {
    open_collection(&state, &id)?.remove_runtime_var(&name);
    Ok(StatusCode::NO_CONTENT)
}

/// Removes every runtime variable of the collection.
pub async fn clear_runtime_vars(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
) -> Result<StatusCode, ApiError> {
    open_collection(&state, &id)?.clear_runtime();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeVarsQuery {
    #[serde(default)]
    pub env: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeVarsDto {
    /// Sorted by name; `source` is always `runtime`.
    pub vars: Vec<VarDto>,
}

/// Secret values of 4 characters or more that a runtime value may not show.
const MIN_SECRET_VALUE: usize = 4;

/// `GET /collections/{id}/runtime-vars?env=`: the runtime variables, masked when the name
/// is a credential field, the value was sent in a credential header this server session,
/// or the value equals a secret of environment `env`.
pub async fn list_runtime_vars(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiQuery(q): ApiQuery<RuntimeVarsQuery>,
) -> Result<Json<RuntimeVarsDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    let sent = open.sent_credentials();
    let secrets: HashSet<String> = q
        .env
        .as_deref()
        .map(|env| {
            open.secrets(env)
                .into_values()
                .filter(|v| v.len() >= MIN_SECRET_VALUE)
                .collect()
        })
        .unwrap_or_default();
    let vars = open
        .runtime()
        .into_iter()
        .map(|(name, value)| {
            let text = value_text(&value);
            let masked =
                is_credential_field(&name) || sent.contains(&text) || secrets.contains(&text);
            let (value, truncated) = shown_value(text, masked);
            VarDto {
                name,
                source: source_name(VarSource::Runtime),
                value,
                masked,
                truncated,
                shadows: Vec::new(),
            }
        })
        .collect();
    Ok(Json(RuntimeVarsDto { vars }))
}

/// Longest value text `/vars` returns; longer values are cut on a character boundary.
const MAX_VALUE: usize = 1024;

/// The text a variable list shows: nothing when masked, else cut at `MAX_VALUE` bytes on
/// a character boundary (with whether it was cut).
fn shown_value(text: String, masked: bool) -> (Option<String>, bool) {
    if masked {
        (None, false)
    } else if text.len() > MAX_VALUE {
        let mut cut = MAX_VALUE;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        (Some(text[..cut].to_owned()), true)
    } else {
        (Some(text), false)
    }
}

/// `POST /collections/{id}/vars`: the same request selection as Send.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VarsBody {
    #[serde(default)]
    pub item: Option<String>,
    #[serde(default)]
    pub draft: Option<HttpRequestDto>,
    #[serde(default)]
    pub env: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VarDto {
    pub name: String,
    /// `processEnv`, `iteration`, `runtime`, `request`, `folder`, `environment` or `collection`.
    pub source: String,
    /// `null` when masked.
    pub value: Option<String>,
    pub masked: bool,
    pub truncated: bool,
    /// Lower scopes that also define the name.
    pub shadows: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetLaterDto {
    pub name: String,
    /// The first request or folder file whose scripts set it.
    pub by: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VarsDto {
    pub vars: Vec<VarDto>,
    /// Names no scope defines yet that scripts set at runtime.
    pub set_later: Vec<SetLaterDto>,
}

fn source_name(source: VarSource) -> String {
    match serde_json::to_value(source) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

/// Explains the variables a send would start with. Runs no script, sends nothing,
/// writes nothing. Masked: environment secrets, process env values, and values this
/// server session sent in a credential header.
pub async fn explain(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<VarsBody>,
) -> Result<Json<VarsDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    if body.item.is_none() && body.draft.is_none() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_item",
            "a scratch request needs a draft",
        ));
    }
    let draft = body.draft.map(HttpRequest::from);
    let input = SendInput {
        item_id: body.item.as_deref(),
        draft: draft.as_ref(),
        env: body.env.as_deref(),
    };
    let stack = preview_stack(&open, &input)?;
    let mut secret_names = HashSet::new();
    if let Some(env) = body.env.as_deref() {
        if let Some(e) = open.collection().environment(env) {
            secret_names.extend(e.file.secrets.iter().map(|s| s.name.clone()));
        }
        secret_names.extend(open.secrets(env).into_keys());
    }
    let sent = open.sent_credentials();
    let vars: Vec<VarDto> = stack
        .entries()
        .into_iter()
        .map(|e| {
            let text = value_text(&e.value);
            let masked = match e.source {
                VarSource::ProcessEnv => true,
                VarSource::Environment if secret_names.contains(&e.name) => true,
                _ => sent.contains(&text),
            };
            let (value, truncated) = shown_value(text, masked);
            VarDto {
                name: e.name,
                source: source_name(e.source),
                value,
                masked,
                truncated,
                shadows: e.shadows.into_iter().map(source_name).collect(),
            }
        })
        .collect();
    let defined: HashSet<&str> = vars.iter().map(|v| v.name.as_str()).collect();
    let set_later = open
        .set_later()
        .iter()
        .filter(|s| !defined.contains(s.name.as_str()))
        .map(|s| SetLaterDto {
            name: s.name.clone(),
            by: s.by.clone(),
        })
        .collect();
    Ok(Json(VarsDto { vars, set_later }))
}
