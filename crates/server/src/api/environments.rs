use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use brs_core::bru::model::EnvironmentFile;
use brs_core::request::KeyValue;
use brs_core::store::{self, ContentHash};

use super::dto::{EnvironmentDto, HashDto, KeyValueDto, SaveEnvironmentBody, SecretsBody};
use super::open_collection;
use crate::error::ApiError;
use crate::state::AppState;

/// Environments as currently on disk, each with its content hash.
pub async fn list(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<Vec<EnvironmentDto>>, ApiError> {
    let open = open_collection(&state, &id)?;
    let collection = open.collection();
    let mut out = Vec::new();
    for env in &collection.environments {
        let bytes = std::fs::read(&env.path).map_err(store::StoreError::from)?;
        let hash = ContentHash::of(&bytes);
        let Ok(file) = EnvironmentFile::parse(&String::from_utf8_lossy(&bytes)) else {
            continue;
        };
        out.push(EnvironmentDto {
            name: env.name.clone(),
            vars: file.vars.iter().map(KeyValueDto::from).collect(),
            secret_names: file.secrets.iter().map(|s| s.name.clone()).collect(),
            hash: hash.0,
        });
    }
    Ok(Json(out))
}

pub async fn save(
    State(state): State<AppState>,
    UrlPath((id, name)): UrlPath<(String, String)>,
    Json(body): Json<SaveEnvironmentBody>,
) -> Result<Json<HashDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    let vars: Vec<KeyValue> = body.vars.into_iter().map(KeyValue::from).collect();
    let hash = store::save_environment(&open.root(), &name, &vars, &ContentHash(body.hash))?;
    open.reload()?;
    Ok(Json(HashDto { hash: hash.0 }))
}

/// Secret values live in server memory only; they are never written to disk.
pub async fn set_secrets(
    State(state): State<AppState>,
    UrlPath((id, name)): UrlPath<(String, String)>,
    Json(body): Json<SecretsBody>,
) -> Result<StatusCode, ApiError> {
    let open = open_collection(&state, &id)?;
    if open.collection().environment(&name).is_none() {
        return Err(ApiError::not_found(format!(
            "environment `{name}` not found"
        )));
    }
    open.update_secrets(
        &name,
        body.values.into_iter().map(|(k, v)| (k, Some(v))).collect(),
    );
    Ok(StatusCode::NO_CONTENT)
}
