//! HAR import: preview the entries, plan an import (names only), and import.

use std::collections::BTreeSet;
use std::path::Path;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use plover_core::har::{ExistingSecrets, Har, ImportPlan, parse_har, plan_import, preview};
use plover_core::store::{self, sanitize_name};
use plover_core::workspace::OpenCollection;

use super::dto::{HarImportBody, HarImportResultDto, HarPlanDto, HarPreviewBody, HarRowDto};
use super::extract::{ApiJson, ApiPath};
use super::open_collection;
use crate::error::ApiError;
use crate::state::AppState;

/// The environment secrets go to when the import names none.
pub const IMPORTED_ENV: &str = "Imported";

fn parse(text: &str) -> Result<Har, ApiError> {
    parse_har(text)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "invalid_har", e.to_string()))
}

pub async fn preview_har(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<HarPreviewBody>,
) -> Result<Json<Vec<HarRowDto>>, ApiError> {
    open_collection(&state, &id)?;
    let har = parse(&body.har)?;
    Ok(Json(
        preview(&har)
            .into_iter()
            .map(|r| HarRowDto {
                index: r.index,
                method: r.method,
                url: r.url,
                host: r.host,
                status: r.status,
                mime: r.mime,
                size: r.size,
                has_secrets: r.has_secrets,
                hidden: r.hidden.map(|h| h.as_str().to_owned()),
                duplicates: r.duplicates,
            })
            .collect(),
    ))
}

/// Where the import goes: the folder ID, and whether it still has to be created.
struct Target {
    folder: String,
    create: bool,
}

struct Planned {
    env: String,
    target: Target,
    plan: ImportPlan,
}

fn join_id(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

fn plan_for(open: &OpenCollection, body: &HarImportBody) -> Result<Planned, ApiError> {
    let har = parse(&body.har)?;
    let root = open.root();
    let parent_dir = if body.folder.is_empty() {
        root.clone()
    } else {
        store::resolve_item_path(&root, &body.folder)?
    };
    if !parent_dir.is_dir() {
        return Err(ApiError::not_found(format!(
            "folder `{}` not found",
            body.folder
        )));
    }
    let target = match body.new_folder.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => {
            let stem = sanitize_name(name);
            if stem.is_empty() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "bad_item",
                    "the new folder needs a name",
                ));
            }
            // An existing folder of that name is imported into, not replaced.
            let create = !parent_dir.join(&stem).is_dir();
            Target {
                folder: join_id(&body.folder, &stem),
                create,
            }
        }
        _ => Target {
            folder: body.folder.clone(),
            create: false,
        },
    };
    let (existing_names, start_seq) = if target.create {
        (BTreeSet::new(), 0.0)
    } else {
        let dir = if target.folder.is_empty() {
            root.clone()
        } else {
            store::resolve_item_path(&root, &target.folder)?
        };
        (store::entry_names(&dir), store::next_seq(&dir) - 1.0)
    };
    let env = body
        .env
        .clone()
        .filter(|e| !e.trim().is_empty())
        .unwrap_or_else(|| IMPORTED_ENV.to_owned());
    let plan = plan_import(
        &har,
        &body.entries,
        &existing_names,
        start_seq,
        &existing_secrets(open, &env),
    );
    Ok(Planned { env, target, plan })
}

/// The environment's secret values and the names an import may not take.
fn existing_secrets(open: &OpenCollection, env: &str) -> ExistingSecrets {
    let mut existing = ExistingSecrets::default();
    let collection = open.collection();
    let Some(environment) = collection.environment(env) else {
        return existing;
    };
    let values = open.secrets(env);
    for var in &environment.file.vars {
        existing.plain_names.insert(var.name.clone());
    }
    for secret in &environment.file.secrets {
        match values.get(&secret.name) {
            Some(value) => {
                existing.values.insert(secret.name.clone(), value.clone());
            }
            // A declared secret without a value: its value is unknown, so it is not reused.
            None => {
                existing.plain_names.insert(secret.name.clone());
            }
        }
    }
    for (name, value) in values {
        existing.values.entry(name).or_insert(value);
    }
    existing
}

pub async fn plan_har(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<HarImportBody>,
) -> Result<Json<HarPlanDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    let planned = plan_for(&open, &body)?;
    Ok(Json(HarPlanDto {
        env: planned.env,
        secret_names: planned.plan.new_secrets.keys().cloned().collect(),
        request_names: planned
            .plan
            .requests
            .iter()
            .map(|(name, _)| name.clone())
            .collect(),
        notices: planned.plan.notices,
    }))
}

/// Writes secret values, then the environment's secret names, then the folder, then one
/// file per request. There is no rollback: on the first failure the result names the
/// step (`failedAt`) and lists the files already written.
pub async fn import_har(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<HarImportBody>,
) -> Result<Json<HarImportResultDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    let Planned { env, target, plan } = plan_for(&open, &body)?;
    let root = open.root();
    let secret_names: Vec<String> = plan.new_secrets.keys().cloned().collect();
    let mut result = HarImportResultDto {
        items: Vec::new(),
        secret_names: secret_names.clone(),
        env: env.clone(),
        notices: plan.notices.clone(),
        failed_at: None,
    };
    let failed = write_import(&open, &root, &env, &target, plan, &mut result.items).err();
    result.failed_at = failed;
    if let Err(e) = open.reload() {
        tracing::warn!("cannot reload the collection after a HAR import: {e}");
    }
    Ok(Json(result))
}

fn write_import(
    open: &OpenCollection,
    root: &Path,
    env: &str,
    target: &Target,
    plan: ImportPlan,
    items: &mut Vec<String>,
) -> Result<(), String> {
    if !plan.new_secrets.is_empty() {
        let names: Vec<String> = plan.new_secrets.keys().cloned().collect();
        let values = plan
            .new_secrets
            .into_iter()
            .map(|(name, value)| (name, Some(value)))
            .collect();
        open.update_secrets(env, values)
            .map_err(|e| format!("secret values: {e}"))?;
        store::add_secret_names(root, env, &names)
            .map_err(|e| format!("environment `{env}`: {e}"))?;
    }
    if target.create {
        let (parent, name) = target
            .folder
            .rsplit_once('/')
            .unwrap_or(("", target.folder.as_str()));
        store::create_folder(root, parent, name)
            .map_err(|e| format!("folder `{}`: {e}", target.folder))?;
    }
    for (name, request) in plan.requests {
        let (item, _) = store::create_request(root, &target.folder, &name, &request)
            .map_err(|e| format!("request `{name}`: {e}"))?;
        items.push(item);
    }
    Ok(())
}
