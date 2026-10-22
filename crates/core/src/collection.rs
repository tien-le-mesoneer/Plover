//! Loads a Bruno collection directory: `bruno.json`, `collection.bru`,
//! environments and the folder/request tree in Bruno's run order.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bru::model::{EnvironmentFile, RequestFile, ScopeFile};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct BrunoConfig {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct Collection {
    pub root: PathBuf,
    pub config: BrunoConfig,
    pub scope: Option<ScopeFile>,
    pub environments: Vec<Environment>,
    pub tree: Folder,
}

#[derive(Debug, Clone)]
pub struct Environment {
    pub name: String,
    pub path: PathBuf,
    pub file: EnvironmentFile,
}

#[derive(Debug, Clone)]
pub struct Folder {
    pub path: PathBuf,
    pub name: String,
    /// `meta.seq` from `folder.bru`, when present.
    pub seq: Option<f64>,
    pub scope: Option<ScopeFile>,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Folder(Box<Folder>),
    Request {
        path: PathBuf,
        file: Box<RequestFile>,
    },
    /// A `.bru` file that could not be read or parsed; Bruno skips these with a warning.
    Invalid {
        path: PathBuf,
        error: String,
    },
}

/// A runnable item with the folders above it (outermost first, root excluded).
#[derive(Debug, Clone)]
pub struct RequestRef<'a> {
    pub path: &'a Path,
    pub item: &'a Item,
    pub folders: Vec<&'a Folder>,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{0} is not inside a Bruno collection (no bruno.json found)")]
    NotACollection(PathBuf),
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid bruno.json at {path}: {source}")]
    Config {
        path: PathBuf,
        source: serde_json::Error,
    },
}

const ALWAYS_IGNORED: &[&str] = &["node_modules", ".git"];

impl Collection {
    /// Walks up from `start` to the directory containing `bruno.json`.
    pub fn find_root(start: &Path) -> Result<PathBuf, LoadError> {
        let start = absolute(start)?;
        let mut dir = if start.is_dir() {
            Some(start.as_path())
        } else {
            start.parent()
        };
        while let Some(d) = dir {
            if d.join("bruno.json").is_file() {
                return Ok(d.to_path_buf());
            }
            dir = d.parent();
        }
        Err(LoadError::NotACollection(start))
    }

    pub fn load(root: &Path) -> Result<Collection, LoadError> {
        let root = absolute(root)?;
        let config_path = root.join("bruno.json");
        let config_text = read(&config_path)?;
        let config: BrunoConfig =
            serde_json::from_str(&config_text).map_err(|source| LoadError::Config {
                path: config_path,
                source,
            })?;
        let scope = load_scope(&root.join("collection.bru"));
        let environments = load_environments(&root.join("environments"))?;
        let tree = load_folder(&root, &root, &config.ignore)?;
        Ok(Collection {
            root,
            config,
            scope,
            environments,
            tree,
        })
    }

    pub fn environment(&self, name: &str) -> Option<&Environment> {
        self.environments.iter().find(|e| e.name == name)
    }

    /// Runnable items at or below `target` (the root, a folder, or one `.bru` file), in run order.
    pub fn requests_under(&self, target: &Path) -> Vec<RequestRef<'_>> {
        let Ok(target) = absolute(target) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        collect(
            &self.tree,
            &mut Vec::new(),
            &target,
            target == self.root,
            &mut out,
        );
        out
    }
}

fn collect<'a>(
    folder: &'a Folder,
    ancestors: &mut Vec<&'a Folder>,
    target: &Path,
    inside: bool,
    out: &mut Vec<RequestRef<'a>>,
) {
    for item in &folder.items {
        match item {
            Item::Folder(sub) => {
                ancestors.push(sub);
                collect(sub, ancestors, target, inside || sub.path == target, out);
                ancestors.pop();
            }
            Item::Request { path, .. } | Item::Invalid { path, .. } => {
                if inside || path == target {
                    out.push(RequestRef {
                        path,
                        item,
                        folders: ancestors.clone(),
                    });
                }
            }
        }
    }
}

fn absolute(path: &Path) -> Result<PathBuf, LoadError> {
    std::path::absolute(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn read(path: &Path) -> Result<String, LoadError> {
    std::fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn load_scope(path: &Path) -> Option<ScopeFile> {
    let text = std::fs::read_to_string(path).ok()?;
    match ScopeFile::parse(&text) {
        Ok(scope) => Some(scope),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "skipping invalid scope file");
            None
        }
    }
}

fn load_environments(dir: &Path) -> Result<Vec<Environment>, LoadError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut envs = Vec::new();
    for path in sorted_entries(dir)? {
        if path.extension().is_none_or(|e| e != "bru") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        match EnvironmentFile::parse(&read(&path)?) {
            Ok(file) => envs.push(Environment { name, path, file }),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "skipping invalid environment")
            }
        }
    }
    Ok(envs)
}

fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>, LoadError> {
    let io = |source| LoadError::Io {
        path: dir.to_path_buf(),
        source,
    };
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(io)? {
        paths.push(entry.map_err(io)?.path());
    }
    paths.sort();
    Ok(paths)
}

fn load_folder(root: &Path, dir: &Path, ignore: &[String]) -> Result<Folder, LoadError> {
    let mut folders = Vec::new();
    let mut requests = Vec::new();
    let mut invalid = Vec::new();
    for path in sorted_entries(dir)? {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            let ignored = file_name.starts_with('.')
                || ALWAYS_IGNORED.contains(&file_name)
                || (dir == root && file_name == "environments")
                || ignore.iter().any(|i| {
                    i == file_name || path.strip_prefix(root).is_ok_and(|r| r == Path::new(i))
                });
            if !ignored {
                folders.push(load_folder(root, &path, ignore)?);
            }
            continue;
        }
        if path.extension().is_none_or(|e| e != "bru")
            || file_name == "collection.bru"
            || file_name == "folder.bru"
        {
            continue;
        }
        let parsed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| RequestFile::parse(&text).map_err(|e| e.to_string()));
        match parsed {
            Ok(file) => requests.push((
                file.request.seq,
                Item::Request {
                    path,
                    file: Box::new(file),
                },
            )),
            Err(error) => invalid.push(Item::Invalid { path, error }),
        }
    }
    // Bruno `utils/collection.js`: folders (name, then seq) first, then requests by seq.
    requests.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
    let mut items: Vec<Item> = sort_folders(folders)
        .into_iter()
        .map(|f| Item::Folder(Box::new(f)))
        .collect();
    items.extend(requests.into_iter().map(|(_, item)| item));
    items.extend(invalid);

    let scope = if dir == root {
        None
    } else {
        load_scope(&dir.join("folder.bru"))
    };
    let dir_name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_owned();
    Ok(Folder {
        path: dir.to_path_buf(),
        name: scope
            .as_ref()
            .and_then(|s| s.name.clone())
            .unwrap_or(dir_name),
        seq: scope.as_ref().and_then(|s| s.seq),
        scope,
        items,
    })
}

/// Port of Bruno `sortByNameThenSequence`: alphabetical by directory name, then
/// each folder with a positive integer seq is inserted at position `seq - 1`.
fn sort_folders(mut folders: Vec<Folder>) -> Vec<Folder> {
    let dir_name = |f: &Folder| {
        f.path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    folders.sort_by_key(dir_name);
    let valid = |f: &Folder| f.seq.is_some_and(|s| s.fract() == 0.0 && s > 0.0);
    let (mut with_seq, without): (Vec<Folder>, Vec<Folder>) = folders.into_iter().partition(valid);
    with_seq.sort_by(|a, b| a.seq.partial_cmp(&b.seq).unwrap_or(Ordering::Equal));
    let mut slots: Vec<Vec<Folder>> = without.into_iter().map(|f| vec![f]).collect();
    for folder in with_seq {
        let position = folder.seq.map(|s| s as usize - 1).unwrap_or(0);
        match slots.get_mut(position) {
            Some(slot) if slot[0].seq == folder.seq => slot.push(folder),
            _ => slots.insert(position.min(slots.len()), vec![folder]),
        }
    }
    slots.into_iter().flatten().collect()
}
