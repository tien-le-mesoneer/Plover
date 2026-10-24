//! Saved runs: `<state_dir>/runs/<collection id>/<run id>.json` holds a whole masked run and
//! `<run id>.meta.json` its summary, written last so a run is listed only once complete.
//! Each run has its own files, so concurrent runs (the app and `plv`) never share one.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::run_record::{RUN_RECORD_VERSION, RunHeader, RunId, RunOutcome, RunRecord};
use crate::runner::RunReport;
use crate::workspace::CollectionId;

/// Runs kept per collection; older ones are deleted after each save.
pub const KEEP_RUNS: usize = 20;
const RUN_EXT: &str = ".json";
const META_EXT: &str = ".meta.json";

/// What listing runs needs, without the results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMeta {
    pub version: u32,
    #[serde(flatten)]
    pub header: RunHeader,
    pub outcome: RunOutcome,
    pub report: RunReport,
}

impl RunMeta {
    pub fn of(record: &RunRecord) -> RunMeta {
        RunMeta {
            version: record.version,
            header: record.header.clone(),
            outcome: record.outcome,
            report: record.report,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("run {0} not found")]
    NotFound(String),
    #[error("cannot read run {id}: {reason}")]
    Unreadable { id: String, reason: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct RunHistory {
    store: Store,
}

enum Store {
    Dir(PathBuf),
    /// For a server without a state directory: same rules, gone on exit.
    Memory(Mutex<HashMap<CollectionId, BTreeMap<RunId, RunRecord>>>),
}

impl RunHistory {
    /// History under `<state_dir>/runs`.
    pub fn at(state_dir: &Path) -> RunHistory {
        RunHistory {
            store: Store::Dir(state_dir.join("runs")),
        }
    }

    pub fn in_memory() -> RunHistory {
        RunHistory {
            store: Store::Memory(Mutex::default()),
        }
    }

    /// Writes the run, then its summary, then deletes runs beyond `KEEP_RUNS`.
    pub fn save(&self, record: &RunRecord) -> Result<(), HistoryError> {
        let key = CollectionId::of_canonical(&record.header.collection_root);
        match &self.store {
            Store::Dir(dir) => {
                let dir = dir.join(&key.0);
                std::fs::create_dir_all(&dir)?;
                let id = record.header.id.as_str();
                let run = serde_json::to_vec(record).map_err(std::io::Error::other)?;
                let meta =
                    serde_json::to_vec(&RunMeta::of(record)).map_err(std::io::Error::other)?;
                write(&dir.join(format!("{id}{RUN_EXT}")), &run)?;
                write(&dir.join(format!("{id}{META_EXT}")), &meta)?;
                // The run is saved; a prune that fails (a file another process holds open)
                // is retried by the next save.
                if let Err(e) = prune(&dir) {
                    tracing::warn!("cannot prune run history in {}: {e}", dir.display());
                }
            }
            Store::Memory(runs) => {
                let mut runs = runs.lock().unwrap_or_else(|e| e.into_inner());
                let runs = runs.entry(key).or_default();
                runs.insert(record.header.id.clone(), record.clone());
                while runs.len() > KEEP_RUNS {
                    runs.pop_first();
                }
            }
        }
        Ok(())
    }

    /// Newest first; unreadable or unknown-version runs are skipped.
    pub fn list(&self, collection: &CollectionId) -> Vec<RunMeta> {
        match &self.store {
            Store::Dir(dir) => {
                let dir = dir.join(&collection.0);
                let mut metas: Vec<RunMeta> = run_ids(&dir)
                    .into_iter()
                    .rev()
                    .filter_map(|id| {
                        let bytes = std::fs::read(dir.join(format!("{id}{META_EXT}"))).ok()?;
                        let meta: RunMeta = serde_json::from_slice(&bytes).ok()?;
                        (meta.version == RUN_RECORD_VERSION && meta.header.id == id).then_some(meta)
                    })
                    .collect();
                metas.sort_by(|a, b| b.header.id.cmp(&a.header.id));
                metas
            }
            Store::Memory(runs) => runs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(collection)
                .map(|runs| runs.values().rev().map(RunMeta::of).collect())
                .unwrap_or_default(),
        }
    }

    pub fn load(&self, collection: &CollectionId, id: &RunId) -> Result<RunRecord, HistoryError> {
        match &self.store {
            Store::Dir(dir) => read_run(&dir.join(&collection.0), id),
            Store::Memory(runs) => runs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(collection)
                .and_then(|runs| runs.get(id).cloned())
                .ok_or_else(|| HistoryError::NotFound(id.to_string())),
        }
    }

    /// Looks in every collection's history (`plv runs show` after a collection moved).
    pub fn find(&self, id: &RunId) -> Result<RunRecord, HistoryError> {
        match &self.store {
            Store::Dir(dir) => {
                let entries = match std::fs::read_dir(dir) {
                    Ok(entries) => entries,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        return Err(HistoryError::NotFound(id.to_string()));
                    }
                    Err(e) => return Err(e.into()),
                };
                for entry in entries.flatten() {
                    let folder = entry.path();
                    if folder.join(format!("{id}{RUN_EXT}")).is_file() {
                        return read_run(&folder, id);
                    }
                }
                Err(HistoryError::NotFound(id.to_string()))
            }
            Store::Memory(runs) => runs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .find_map(|runs| runs.get(id).cloned())
                .ok_or_else(|| HistoryError::NotFound(id.to_string())),
        }
    }

    /// Deletes the collection's runs; returns how many.
    pub fn clear(&self, collection: &CollectionId) -> Result<usize, HistoryError> {
        match &self.store {
            Store::Dir(dir) => {
                let dir = dir.join(&collection.0);
                let ids = run_ids(&dir);
                for id in &ids {
                    remove(&dir, id)?;
                }
                Ok(ids.len())
            }
            Store::Memory(runs) => Ok(runs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(collection)
                .map(|r| r.len())
                .unwrap_or_default()),
        }
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), HistoryError> {
    crate::store::write_atomic(path, bytes)
        .map(|_| ())
        .map_err(|e| match e {
            crate::store::StoreError::Io(io) => HistoryError::Io(io),
            other => HistoryError::Io(std::io::Error::other(other.to_string())),
        })
}

fn read_run(dir: &Path, id: &RunId) -> Result<RunRecord, HistoryError> {
    let bytes = match std::fs::read(dir.join(format!("{id}{RUN_EXT}"))) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(HistoryError::NotFound(id.to_string()));
        }
        Err(e) => return Err(e.into()),
    };
    let unreadable = |reason: String| HistoryError::Unreadable {
        id: id.to_string(),
        reason,
    };
    let version = serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|e| unreadable(e.to_string()))?
        .get("version")
        .and_then(|v| v.as_u64());
    if version != Some(u64::from(RUN_RECORD_VERSION)) {
        return Err(unreadable(format!(
            "unsupported version {}",
            version.map_or_else(|| "none".to_owned(), |v| v.to_string())
        )));
    }
    serde_json::from_slice(&bytes).map_err(|e| unreadable(e.to_string()))
}

/// Run IDs that have any file in `dir`, oldest first. Files whose names are not run IDs
/// are ignored.
fn run_ids(dir: &Path) -> Vec<RunId> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let ids: BTreeSet<RunId> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let stem = name
                .strip_suffix(META_EXT)
                .or_else(|| name.strip_suffix(RUN_EXT))?;
            RunId::parse(stem)
        })
        .collect();
    ids.into_iter().collect()
}

fn prune(dir: &Path) -> Result<(), HistoryError> {
    let ids = run_ids(dir);
    let extra = ids.len().saturating_sub(KEEP_RUNS);
    for id in &ids[..extra] {
        remove(dir, id)?;
    }
    Ok(())
}

/// Removes both files of a run; one that is already gone (a concurrent prune) is fine.
fn remove(dir: &Path, id: &RunId) -> Result<(), HistoryError> {
    for ext in [META_EXT, RUN_EXT] {
        match std::fs::remove_file(dir.join(format!("{id}{ext}"))) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
