//! Collections opened by a long-lived process (the server): each keeps its
//! loaded tree, one HTTP session (cookie jar), runtime variables and the
//! secret environment values entered for it, which are never written to disk.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use crate::collection::{Collection, LoadError};
use crate::executor::{ExecError, ExecOptions, Session};
use crate::store::ContentHash;
use crate::vars::{ValueMap, VarMap};

/// First 16 hex characters of the SHA-256 of the canonical collection root.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CollectionId(pub String);

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error(transparent)]
    Load(#[from] LoadError),
    #[error(transparent)]
    Session(#[from] ExecError),
}

pub struct OpenCollection {
    pub id: CollectionId,
    collection: RwLock<Arc<Collection>>,
    pub session: Session,
    runtime: Mutex<ValueMap>,
    secrets: Mutex<BTreeMap<String, VarMap>>,
    /// Environment variables scripts set (`bru.setEnvVar`), per environment, in memory only.
    env_changes: Mutex<BTreeMap<String, ValueMap>>,
}

impl OpenCollection {
    /// A snapshot of the loaded collection; cheap to clone, safe to hold across awaits.
    pub fn collection(&self) -> Arc<Collection> {
        self.collection
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn root(&self) -> PathBuf {
        self.collection().root.clone()
    }

    /// Re-reads the collection from disk, keeping the session, runtime vars and secrets.
    pub fn reload(&self) -> Result<(), LoadError> {
        let fresh = Collection::load(&self.root())?;
        *self
            .collection
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Arc::new(fresh);
        Ok(())
    }

    pub fn runtime(&self) -> ValueMap {
        self.runtime
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set_runtime(&self, vars: ValueMap) {
        *self.runtime.lock().unwrap_or_else(PoisonError::into_inner) = vars;
    }

    /// Applies what one send or run changed in the runtime variables (`before` → `after`):
    /// values it set or changed, and names it deleted. Changes other sends or runs made
    /// meanwhile are kept.
    pub fn record_runtime_changes(&self, before: &ValueMap, after: &ValueMap) {
        let mut runtime = self.runtime.lock().unwrap_or_else(PoisonError::into_inner);
        apply_changes(&mut runtime, before, after);
    }

    /// What overrides environment `env`'s file values: secrets entered for it, then
    /// values scripts set earlier in this session.
    pub fn env_overrides(&self, env: &str) -> ValueMap {
        let mut values = crate::vars::string_values(&self.secrets(env));
        if let Some(changes) = self
            .env_changes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(env)
        {
            values.extend(changes.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        values
    }

    /// Remembers what scripts changed in environment `env` (`before` → `after`): new or
    /// changed values are kept for later sends and runs; a variable a script deleted
    /// stops being overridden (its file value, if any, applies again).
    pub fn record_env_changes(&self, env: &str, before: &ValueMap, after: &ValueMap) {
        let mut all = self
            .env_changes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let changes = all.entry(env.to_owned()).or_default();
        for (name, value) in after {
            if before.get(name) != Some(value) {
                changes.insert(name.clone(), value.clone());
            }
        }
        for name in before.keys().filter(|k| !after.contains_key(*k)) {
            changes.remove(name);
        }
    }

    pub fn secrets(&self, env: &str) -> VarMap {
        self.secrets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(env)
            .cloned()
            .unwrap_or_default()
    }

    /// Merges secret values for environment `env`: `Some` sets a value, `None` clears it.
    /// Names not mentioned keep their current value.
    pub fn update_secrets(&self, env: &str, changes: BTreeMap<String, Option<String>>) {
        let mut secrets = self.secrets.lock().unwrap_or_else(PoisonError::into_inner);
        let values = secrets.entry(env.to_owned()).or_default();
        for (name, value) in changes {
            match value {
                Some(v) => values.insert(name, v),
                None => values.remove(&name),
            };
        }
    }

    /// Names (never values) of the secrets that currently have a value, sorted.
    pub fn secret_names_set(&self, env: &str) -> Vec<String> {
        self.secrets(env).into_keys().collect()
    }
}

/// Sets the values `after` has that differ from `before`, and removes names `before` had
/// that `after` lacks; everything else in `target` stays.
fn apply_changes(target: &mut ValueMap, before: &ValueMap, after: &ValueMap) {
    for (name, value) in after {
        if before.get(name) != Some(value) {
            target.insert(name.clone(), value.clone());
        }
    }
    for name in before.keys().filter(|k| !after.contains_key(*k)) {
        target.remove(name);
    }
}

pub struct Workspace {
    exec: ExecOptions,
    collections: RwLock<HashMap<CollectionId, Arc<OpenCollection>>>,
}

impl Workspace {
    pub fn new(exec: ExecOptions) -> Workspace {
        Workspace {
            exec,
            collections: RwLock::default(),
        }
    }

    /// Opens the collection containing `path` (its root, a folder or a file).
    /// Opening an already open collection returns its existing ID.
    pub fn open(&self, path: &Path) -> Result<CollectionId, WorkspaceError> {
        let root = Collection::find_root(path)?;
        let canonical = root.canonicalize().map_err(|source| LoadError::Io {
            path: root.clone(),
            source,
        })?;
        let id = CollectionId(
            ContentHash::of(canonical.to_string_lossy().as_bytes()).0[..16].to_owned(),
        );
        if self.get(&id).is_some() {
            return Ok(id);
        }
        let collection = Collection::load(&root)?;
        let open = Arc::new(OpenCollection {
            id: id.clone(),
            collection: RwLock::new(Arc::new(collection)),
            session: Session::new(self.exec.clone())?,
            runtime: Mutex::default(),
            secrets: Mutex::default(),
            env_changes: Mutex::default(),
        });
        self.collections
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(id.clone())
            .or_insert(open);
        Ok(id)
    }

    pub fn get(&self, id: &CollectionId) -> Option<Arc<OpenCollection>> {
        self.collections
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .cloned()
    }

    /// Open collections, ordered by ID.
    pub fn list(&self) -> Vec<Arc<OpenCollection>> {
        let mut all: Vec<_> = self
            .collections
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        all.sort_by(|a, b| a.id.cmp(&b.id));
        all
    }
}
