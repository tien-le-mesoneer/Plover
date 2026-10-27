//! "Copy value": `{{variables}}` in a piece of request text filled in the way Send would
//! fill them (no script runs). A result that contains a secret comes back as `null`
//! unless the caller asks again with `reveal: true`, so the page never holds a secret
//! the user did not choose to copy.

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use plover_core::request::HttpRequest;
use plover_core::run_record::MAX_DETAIL_BODY;
use plover_core::runner::{SendInput, preview_stack};
use plover_core::vars::{VarSource, value_text};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::dto::HttpRequestDto;
use super::extract::{ApiJson, ApiPath};
use super::open_collection;
use crate::error::ApiError;
use crate::state::AppState;

/// Secret values shorter than this are only caught where a placeholder names them.
const MIN_SECRET_VALUE: usize = 4;

/// `POST /collections/{id}/resolve-text`: the same request selection as Send.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResolveTextBody {
    #[serde(default)]
    pub item: Option<String>,
    #[serde(default)]
    pub draft: Option<HttpRequestDto>,
    #[serde(default)]
    pub env: Option<String>,
    /// At most 1 MiB.
    pub text: String,
    /// Return the text even when it contains a secret.
    #[serde(default)]
    pub reveal: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResolveTextDto {
    /// `null` when the text contains a secret and `reveal` was false.
    pub text: Option<String>,
    pub contains_secret: bool,
}

/// The names of `{{name}}` placeholders, as core finds them (up to the first `}`, which
/// must start `}}`).
fn placeholder_names(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(start) = text[from..].find("{{").map(|i| i + from) {
        match text[start + 2..].find('}').map(|i| i + start + 2) {
            Some(close) if text[close..].starts_with("}}") => {
                let name = text[start + 2..close].trim();
                if !name.is_empty() {
                    out.push(name);
                }
                from = close + 2;
            }
            _ => from = start + 1,
        }
    }
    out
}

pub async fn resolve_text(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<ResolveTextBody>,
) -> Result<Json<ResolveTextDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    if body.item.is_none() && body.draft.is_none() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_item",
            "a scratch request needs a draft",
        ));
    }
    if body.text.len() > MAX_DETAIL_BODY {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "text_too_large",
            "text to resolve is limited to 1 MiB",
        ));
    }
    let draft = body.draft.map(HttpRequest::from);
    let input = SendInput {
        item_id: body.item.as_deref(),
        draft: draft.as_ref(),
        env: body.env.as_deref(),
    };
    let stack = preview_stack(&open, &input)?;
    // The same masking source as the variable hover (`vars::explain`).
    let mut secret_names = HashSet::new();
    let mut env_secrets = Vec::new();
    if let Some(env) = body.env.as_deref() {
        if let Some(e) = open.collection().environment(env) {
            secret_names.extend(e.file.secrets.iter().map(|s| s.name.clone()));
        }
        let secrets = open.secrets(env);
        secret_names.extend(secrets.keys().cloned());
        env_secrets.extend(secrets.into_values());
    }
    let sent = open.sent_credentials();
    let mut hidden_names = HashSet::new();
    let mut hidden_values = env_secrets;
    for e in stack.entries() {
        let text = value_text(&e.value);
        let masked = match e.source {
            VarSource::ProcessEnv => true,
            VarSource::Environment if secret_names.contains(&e.name) => true,
            _ => sent.contains(&text),
        };
        if masked {
            hidden_names.insert(e.name);
            hidden_values.push(text);
        }
    }
    let root = |name: &str| name.split(['.', '[']).next().unwrap_or(name).to_owned();
    let names_secret = placeholder_names(&body.text)
        .into_iter()
        .any(|n| hidden_names.contains(n) || hidden_names.contains(&root(n)));
    let text = stack.interpolate(&body.text);
    let contains_secret = names_secret
        || hidden_values
            .iter()
            .any(|v| v.len() >= MIN_SECRET_VALUE && text.contains(v.as_str()));
    Ok(Json(ResolveTextDto {
        text: (body.reveal || !contains_secret).then_some(text),
        contains_secret,
    }))
}
