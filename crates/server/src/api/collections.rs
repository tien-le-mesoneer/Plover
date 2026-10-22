use std::path::{Path, PathBuf};

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use brs_core::collection::{Folder, Item};
use brs_core::workspace::OpenCollection;

use super::dto::{CollectionSummaryDto, OpenCollectionBody, TreeNodeDto, TreeNodeKind};
use super::open_collection;
use crate::error::ApiError;
use crate::state::AppState;

fn summary(open: &OpenCollection) -> CollectionSummaryDto {
    let collection = open.collection();
    CollectionSummaryDto {
        id: open.id.0.clone(),
        name: collection.config.name.clone(),
        root: collection.root.display().to_string(),
    }
}

pub async fn open(
    State(state): State<AppState>,
    Json(body): Json<OpenCollectionBody>,
) -> Result<Json<CollectionSummaryDto>, ApiError> {
    let workspace = state.workspace.clone();
    let path = PathBuf::from(body.path);
    // Loading reads many files; keep it off the async workers.
    let id = tokio::task::spawn_blocking(move || workspace.open(&path))
        .await
        .map_err(|e| {
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                e.to_string(),
            )
        })??;
    let open = open_collection(&state, &id.0)?;
    Ok(Json(summary(&open)))
}

pub async fn list(State(state): State<AppState>) -> Json<Vec<CollectionSummaryDto>> {
    Json(state.workspace.list().iter().map(|o| summary(o)).collect())
}

pub async fn tree(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<TreeNodeDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    let collection = open.collection();
    Ok(Json(folder_node(
        &collection.root,
        &collection.tree,
        &collection.config.name,
    )))
}

pub async fn reload(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<TreeNodeDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    let reloading = open.clone();
    tokio::task::spawn_blocking(move || reloading.reload())
        .await
        .map_err(|e| {
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                e.to_string(),
            )
        })??;
    let collection = open.collection();
    Ok(Json(folder_node(
        &collection.root,
        &collection.tree,
        &collection.config.name,
    )))
}

/// `/`-separated path of `path` relative to `root`.
pub fn item_id(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn folder_node(root: &Path, folder: &Folder, root_name: &str) -> TreeNodeDto {
    let children = folder
        .items
        .iter()
        .map(|item| match item {
            Item::Folder(sub) => folder_node(root, sub, root_name),
            Item::Request { path, file } => TreeNodeDto {
                kind: TreeNodeKind::Request,
                id: item_id(root, path),
                name: file.request.name.clone(),
                method: Some(file.request.method.as_str().to_owned()),
                seq: Some(file.request.seq),
                error: None,
                children: None,
            },
            Item::Invalid { path, error } => TreeNodeDto {
                kind: TreeNodeKind::Invalid,
                id: item_id(root, path),
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                method: None,
                seq: None,
                error: Some(error.clone()),
                children: None,
            },
        })
        .collect();
    let is_root = folder.path == root;
    TreeNodeDto {
        kind: TreeNodeKind::Folder,
        id: item_id(root, &folder.path),
        name: if is_root {
            root_name.to_owned()
        } else {
            folder.name.clone()
        },
        method: None,
        seq: folder.seq,
        error: None,
        children: Some(children),
    }
}
