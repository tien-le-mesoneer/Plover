//! Collections opened by a long-lived process (the server): each keeps its
//! loaded tree, one HTTP session (cookie jar) and runtime variables. Secret
//! environment values live in a `SecretStore` (OS keychain or memory), never on disk.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use crate::collection::{Collection, LoadError};
use crate::executor::{ExecError, ExecOptions, Session};
use crate::secrets::{MemoryStore, SecretBackend, SecretError, SecretKey, SecretStore};
use crate::store::ContentHash;
use crate::vars::{ValueMap, VarMap};

/// First 16 hex characters of the SHA-256 of the canonical collection root.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CollectionId(pub String);

impl CollectionId {
    /// The ID of the collection whose root is `root` (also the keychain account prefix).
    pub fn for_root(root: &Path) -> Result<CollectionId, LoadError> {
        let canonical = root.canonicalize().map_err(|source| LoadError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        Ok(CollectionId(
            ContentHash::of(canonical.to_string_lossy().as_bytes()).0[..16].to_owned(),
        ))
    }
}

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
    secrets: Arc<dyn SecretStore>,
    /// Secret names set in this session per environment, besides those the env file declares.
    secret_names: Mutex<BTreeMap<String, BTreeSet<String>>>,
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

    fn secret_key(&self, env: &str, name: &str) -> SecretKey {
        SecretKey {
            collection: self.id.clone(),
            env: env.to_owned(),
            name: name.to_owned(),
        }
    }

    /// Secret names for `env`: those its file declares (`vars:secret`) plus any set this session.
    fn known_secret_names(&self, env: &str) -> BTreeSet<String> {
        let mut names: BTreeSet<String> = self
            .collection()
            .environment(env)
            .map(|e| e.file.secrets.iter().map(|s| s.name.clone()).collect())
            .unwrap_or_default();
        if let Some(extra) = self
            .secret_names
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(env)
        {
            names.extend(extra.iter().cloned());
        }
        names
    }

    /// The stored secret values for `env`. A store error skips that value (logged without it).
    pub fn secrets(&self, env: &str) -> VarMap {
        let mut values = VarMap::new();
        for name in self.known_secret_names(env) {
            match self.secrets.get(&self.secret_key(env, &name)) {
                Ok(Some(v)) => {
                    values.insert(name, v);
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(%name, error = %e, "could not read secret"),
            }
        }
        values
    }

    /// Merges secret values for environment `env`: `Some` sets a value, `None` clears it.
    /// Names not mentioned keep their current value.
    pub fn update_secrets(
        &self,
        env: &str,
        changes: BTreeMap<String, Option<String>>,
    ) -> Result<(), SecretError> {
        for (name, value) in changes {
            let key = self.secret_key(env, &name);
            match value {
                Some(v) => {
                    self.secrets.set(&key, &v)?;
                    self.secret_names
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .entry(env.to_owned())
                        .or_default()
                        .insert(name);
                }
                None => self.secrets.delete(&key)?,
            }
        }
        Ok(())
    }

    /// Names (never values) of the secrets that currently have a value, sorted.
    pub fn secret_names_set(&self, env: &str) -> Vec<String> {
        self.secrets(env).into_keys().collect()
    }

    /// Where this collection's secrets are kept right now.
    pub fn secret_backend(&self) -> SecretBackend {
        self.secrets.kind()
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
    secrets: Arc<dyn SecretStore>,
    collections: RwLock<HashMap<CollectionId, Arc<OpenCollection>>>,
}

impl Workspace {
    /// Secrets are kept in memory; see [`Workspace::with_secrets`] for the keychain.
    pub fn new(exec: ExecOptions) -> Workspace {
        Workspace::with_secrets(exec, Arc::new(MemoryStore::default()))
    }

    pub fn with_secrets(exec: ExecOptions, secrets: Arc<dyn SecretStore>) -> Workspace {
        Workspace {
            exec,
            secrets,
            collections: RwLock::default(),
        }
    }

    pub fn secret_backend(&self) -> SecretBackend {
        self.secrets.kind()
    }

    /// Opens the collection containing `path` (its root, a folder or a file).
    /// Opening an already open collection returns its existing ID.
    pub fn open(&self, path: &Path) -> Result<CollectionId, WorkspaceError> {
        let root = Collection::find_root(path)?;
        let id = CollectionId::for_root(&root)?;
        if self.get(&id).is_some() {
            return Ok(id);
        }
        let collection = Collection::load(&root)?;
        let open = Arc::new(OpenCollection {
            id: id.clone(),
            collection: RwLock::new(Arc::new(collection)),
            session: Session::new(self.exec.clone())?,
            runtime: Mutex::default(),
            secrets: self.secrets.clone(),
            secret_names: Mutex::default(),
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
