//! Collections opened by a long-lived process (the server): each keeps its
//! loaded tree, one HTTP session (cookie jar) and runtime variables. Secret
//! environment values live in a `SecretVault` (OS keychain or memory), never on disk.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};

use crate::app_state::AppStateStore;
use crate::collection::{Collection, LoadError};
use crate::executor::{ExecError, ExecOptions, Session};
use crate::secrets::{SecretBackend, SecretError, SecretKey, SecretVault};
use crate::store::ContentHash;
use crate::var_scan::{SetLater, scan_set_later};
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
        Ok(CollectionId::of_canonical(&canonical.to_string_lossy()))
    }

    /// The ID for an already canonical root string, e.g. a remembered root that no
    /// longer exists (it keeps the ID it had).
    pub fn of_canonical(root: &str) -> CollectionId {
        CollectionId(ContentHash::of(root.as_bytes()).0[..16].to_owned())
    }
}

/// A remembered collection that could not be reopened (moved, deleted, unreadable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unavailable {
    pub id: CollectionId,
    /// The canonical root it had.
    pub root: String,
    pub name: String,
    pub reason: String,
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
    /// The canonical root at open time: the key for remembered state.
    pub canonical_root: String,
    collection: RwLock<Arc<Collection>>,
    pub session: Session,
    runtime: Mutex<ValueMap>,
    secrets: Arc<SecretVault>,
    /// Secret names set in this session per environment, besides those the env file declares.
    secret_names: Mutex<BTreeMap<String, BTreeSet<String>>>,
    /// Environment variables scripts set (`bru.setEnvVar`), per environment, in memory only.
    env_changes: Mutex<BTreeMap<String, ValueMap>>,
    /// Credential header values sent during this server session (memory only), so the
    /// variable hover can mask runtime values that held them.
    sent_credentials: Mutex<HashSet<String>>,
    /// `scan_set_later` of the collection snapshot it was computed for.
    set_later: Mutex<Option<SetLaterCache>>,
}

/// A `scan_set_later` result and the collection snapshot it was computed from.
type SetLaterCache = (Arc<Collection>, Arc<Vec<SetLater>>);

impl OpenCollection {
    /// A snapshot of the loaded collection; cheap to clone, safe to hold across awaits.
    pub fn collection(&self) -> Arc<Collection> {
        self.collection
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Remembers credential values a send or run put in its request headers.
    pub fn note_sent_credentials(&self, values: Vec<String>) {
        if values.is_empty() {
            return;
        }
        self.sent_credentials
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(values);
    }

    pub fn sent_credentials(&self) -> HashSet<String> {
        self.sent_credentials
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Names scripts set at runtime, rescanned when the collection was reloaded.
    pub fn set_later(&self) -> Arc<Vec<SetLater>> {
        let collection = self.collection();
        let mut cached = self
            .set_later
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((of, found)) = cached.as_ref()
            && Arc::ptr_eq(of, &collection)
        {
            return found.clone();
        }
        let found = Arc::new(scan_set_later(&collection));
        *cached = Some((collection, found.clone()));
        found
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

    /// Sets one runtime variable, as `bru.setVar(name, value)` would; the next send or
    /// run sees it.
    pub fn set_runtime_var(&self, name: &str, value: serde_json::Value) {
        self.runtime
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.to_owned(), value);
    }

    /// Removes one runtime variable; `false` when it was not set. A send or run already
    /// going still applies its own changes when it ends.
    pub fn remove_runtime_var(&self, name: &str) -> bool {
        self.runtime
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name)
            .is_some()
    }

    /// Removes every runtime variable. A send or run already going still applies its own
    /// changes when it ends.
    pub fn clear_runtime(&self) {
        self.runtime
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
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

    /// The stored secret values for `env`. An unreadable value is skipped (logged without it).
    pub fn secrets(&self, env: &str) -> VarMap {
        self.stored_secrets(env)
            .into_iter()
            .map(|(name, stored)| (name, stored.value))
            .collect()
    }

    fn stored_secrets(&self, env: &str) -> Vec<(String, crate::secrets::StoredSecret)> {
        let mut out = Vec::new();
        for name in self.known_secret_names(env) {
            match self.secrets.get(&self.secret_key(env, &name)) {
                Ok(Some(stored)) => out.push((name, stored)),
                Ok(None) => {}
                Err(e) => tracing::warn!(%name, error = %e, "could not read secret"),
            }
        }
        out
    }

    /// Merges secret values for environment `env`: `Some` sets a value, `None` clears it
    /// (from memory and the keychain). Names not mentioned keep their current value.
    /// Returns a warning for each value kept in memory instead of the keychain.
    pub fn update_secrets(
        &self,
        env: &str,
        changes: BTreeMap<String, Option<String>>,
    ) -> Result<Vec<String>, SecretError> {
        let mut warnings = Vec::new();
        for (name, value) in changes {
            let key = self.secret_key(env, &name);
            match value {
                Some(v) => {
                    let outcome = self.secrets.set(&key, &v)?;
                    warnings.extend(outcome.warning);
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
        Ok(warnings)
    }

    /// Names (never values) of the secrets that currently have a value, sorted, with
    /// where each is kept.
    pub fn secret_status(&self, env: &str) -> Vec<(String, SecretBackend)> {
        self.stored_secrets(env)
            .into_iter()
            .map(|(name, stored)| (name, stored.backend))
            .collect()
    }

    /// Where new secret values go right now.
    pub fn secret_backend(&self) -> SecretBackend {
        self.secrets.availability()
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
    secrets: Arc<SecretVault>,
    /// Each entry keeps its open sequence number so `list` returns open order.
    collections: RwLock<HashMap<CollectionId, (u64, Arc<OpenCollection>)>>,
    next_seq: AtomicU64,
    /// Where open/close is remembered between starts; `None` remembers nothing.
    app_state: OnceLock<Arc<AppStateStore>>,
    unavailable: Mutex<Vec<Unavailable>>,
}

impl Workspace {
    /// Secrets are kept in memory; see [`Workspace::with_secrets`] for the keychain.
    pub fn new(exec: ExecOptions) -> Workspace {
        Workspace::with_secrets(exec, Arc::new(SecretVault::memory()))
    }

    pub fn with_secrets(exec: ExecOptions, secrets: Arc<SecretVault>) -> Workspace {
        Workspace {
            exec,
            secrets,
            collections: RwLock::default(),
            next_seq: AtomicU64::new(0),
            app_state: OnceLock::new(),
            unavailable: Mutex::default(),
        }
    }

    /// Remembers opened and closed collections (and serves UI state) through `store`.
    pub fn with_app_state(self, store: Arc<AppStateStore>) -> Workspace {
        self.set_app_state(store);
        self
    }

    /// Attaches the store once (e.g. after the workspace is shared); later calls are
    /// ignored and return `false`.
    pub fn set_app_state(&self, store: Arc<AppStateStore>) -> bool {
        self.app_state.set(store).is_ok()
    }

    pub fn app_state(&self) -> Option<&Arc<AppStateStore>> {
        self.app_state.get()
    }

    /// Reopens the remembered collections in their saved order. Those that fail to load
    /// become [`Unavailable`] entries instead of errors.
    pub fn restore(&self) {
        let Some(store) = self.app_state.get() else {
            return;
        };
        let saved = store.snapshot();
        for entry in &saved.open {
            if let Err(e) = self.open_inner(Path::new(&entry.root), false) {
                let name = saved
                    .recent
                    .iter()
                    .find(|r| r.root == entry.root)
                    .map(|r| r.name.clone())
                    .or_else(|| {
                        Path::new(&entry.root)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                    })
                    .unwrap_or_else(|| entry.root.clone());
                let mut missing = self
                    .unavailable
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if !missing.iter().any(|u| u.root == entry.root) {
                    missing.push(Unavailable {
                        id: CollectionId::of_canonical(&entry.root),
                        root: entry.root.clone(),
                        name,
                        reason: e.to_string(),
                    });
                }
            }
        }
    }

    /// Remembered collections that could not be reopened, in saved order.
    pub fn unavailable(&self) -> Vec<Unavailable> {
        self.unavailable
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Closes an open collection, or drops an unavailable one; either way it is not
    /// reopened on the next start (it stays in the recent list). `false` if unknown.
    pub fn close(&self, id: &CollectionId) -> bool {
        let closed = self
            .collections
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id)
            .map(|(_, open)| open.canonical_root.clone());
        let root = closed.or_else(|| {
            let mut missing = self
                .unavailable
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let index = missing.iter().position(|u| &u.id == id)?;
            Some(missing.remove(index).root)
        });
        match root {
            Some(root) => {
                if let Some(store) = self.app_state.get() {
                    store.record_close(&root);
                }
                true
            }
            None => false,
        }
    }

    /// The canonical root of an open or unavailable collection.
    pub fn canonical_root(&self, id: &CollectionId) -> Option<String> {
        if let Some(open) = self.get(id) {
            return Some(open.canonical_root.clone());
        }
        self.unavailable
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|u| &u.id == id)
            .map(|u| u.root.clone())
    }

    pub fn secret_backend(&self) -> SecretBackend {
        self.secrets.availability()
    }

    /// Opens the collection containing `path` (its root, a folder or a file).
    /// Opening an already open collection returns its existing ID.
    pub fn open(&self, path: &Path) -> Result<CollectionId, WorkspaceError> {
        self.open_inner(path, true)
    }

    /// `remember: false` (restoring) keeps the recent list's order as it was.
    fn open_inner(&self, path: &Path, remember: bool) -> Result<CollectionId, WorkspaceError> {
        let root = Collection::find_root(path)?;
        let canonical = root
            .canonicalize()
            .map_err(|source| LoadError::Io {
                path: root.clone(),
                source,
            })?
            .to_string_lossy()
            .into_owned();
        let id = CollectionId::of_canonical(&canonical);
        if self.get(&id).is_some() {
            return Ok(id);
        }
        let collection = Collection::load(&root)?;
        let name = collection.config.name.clone();
        let open = Arc::new(OpenCollection {
            id: id.clone(),
            canonical_root: canonical.clone(),
            collection: RwLock::new(Arc::new(collection)),
            session: Session::new(self.exec.clone())?,
            runtime: Mutex::default(),
            secrets: self.secrets.clone(),
            secret_names: Mutex::default(),
            env_changes: Mutex::default(),
            sent_credentials: Mutex::default(),
            set_later: Mutex::default(),
        });
        self.collections
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(id.clone())
            .or_insert_with(|| (self.next_seq.fetch_add(1, Ordering::Relaxed), open));
        self.unavailable
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|u| u.root != canonical);
        if remember && let Some(store) = self.app_state.get() {
            store.record_open(&canonical, &name);
        }
        Ok(id)
    }

    pub fn get(&self, id: &CollectionId) -> Option<Arc<OpenCollection>> {
        self.collections
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .map(|(_, open)| open.clone())
    }

    /// Open collections in the order they were first opened.
    pub fn list(&self) -> Vec<Arc<OpenCollection>> {
        let mut all: Vec<_> = self
            .collections
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        all.sort_by_key(|(seq, _)| *seq);
        all.into_iter().map(|(_, open)| open).collect()
    }
}
