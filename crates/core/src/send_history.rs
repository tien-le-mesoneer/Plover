//! The last UI sends: `<state_dir>/sends/<id>.json` holds one masked send (request,
//! response, checks) and `<id>.meta.json` its summary, written last so a send is listed
//! only once complete. The newest [`KEEP_SENDS`] are kept across all collections.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::masking::Masker;
use crate::run_history::{HistoryError, remove, run_ids, write};
use crate::run_record::{ResultRecord, ResultStatus, RunId};
use crate::runner::RequestOutcome;
use crate::workspace::CollectionId;

/// Sends kept in total; older ones are deleted after each save.
pub const KEEP_SENDS: usize = 100;
pub const SEND_RECORD_VERSION: u32 = 1;
const SEND_EXT: &str = ".json";
const META_EXT: &str = ".meta.json";

/// Which send this is. A file is self-contained: it names its collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendHeader {
    pub id: RunId,
    pub sent_at_ms: u64,
    /// The canonical collection root.
    pub collection_root: String,
    pub collection_name: String,
    /// The request file, relative to the root; `None` for a scratch request.
    pub item: Option<String>,
    pub env: Option<String>,
}

/// One send as history stores it: masked before it is written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendRecord {
    pub version: u32,
    #[serde(flatten)]
    pub header: SendHeader,
    pub result: ResultRecord,
}

/// What listing sends needs, without bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMeta {
    pub version: u32,
    #[serde(flatten)]
    pub header: SendHeader,
    pub method: String,
    pub name: String,
    pub url: String,
    pub status: ResultStatus,
    pub http_status: Option<u16>,
    pub duration_ms: u64,
}

impl SendMeta {
    pub fn of(record: &SendRecord) -> SendMeta {
        let r = &record.result;
        SendMeta {
            version: record.version,
            header: record.header.clone(),
            method: r.method.clone(),
            name: r.name.clone(),
            url: r.url.clone(),
            status: r.status,
            http_status: r.outcome.status,
            duration_ms: u64::try_from(r.duration().as_millis()).unwrap_or(u64::MAX),
        }
    }
}

impl SendRecord {
    /// The record with `secrets` and every credential header value of the send masked.
    pub fn masked(header: SendHeader, mut result: ResultRecord, secrets: &[String]) -> SendRecord {
        Masker::new(secrets, std::slice::from_ref(&result)).mask_result(&mut result);
        SendRecord {
            version: SEND_RECORD_VERSION,
            header,
            result,
        }
    }
}

fn whole_ms(d: Duration) -> Duration {
    Duration::from_millis(u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// One send as a result record, with Bruno's status rules as runs use them: `error` when
/// there was no response, `fail` when a check or a script failed, else `pass`.
/// `method` and `url` are used when the request was never sent (no `outcome.request`).
pub fn send_result(
    item: Option<&str>,
    name: &str,
    method: &str,
    url: &str,
    outcome: &RequestOutcome,
    error: Option<String>,
) -> ResultRecord {
    let status = if error.is_some() || outcome.status.is_none() {
        ResultStatus::Error
    } else if outcome.failure_summary().is_some() {
        ResultStatus::Fail
    } else {
        ResultStatus::Pass
    };
    let mut outcome = outcome.clone();
    outcome.elapsed = whole_ms(outcome.elapsed);
    outcome.duration = whole_ms(outcome.duration);
    let (method, url) = outcome
        .request
        .as_ref()
        .map_or((method.to_owned(), url.to_owned()), |r| {
            (r.method.clone(), r.url.clone())
        });
    ResultRecord {
        index: 0,
        iteration_index: 0,
        file: item.unwrap_or_default().to_owned(),
        name: name.to_owned(),
        method,
        url,
        status,
        error: error.or_else(|| outcome.failure_summary()),
        skip_reason: None,
        outcome,
    }
}

pub struct SendHistory {
    store: Store,
}

enum Store {
    Dir(PathBuf),
    /// For a server without a state directory: same rules, gone on exit.
    Memory(Mutex<BTreeMap<RunId, SendRecord>>),
}

impl SendHistory {
    /// History under `<state_dir>/sends`.
    pub fn at(state_dir: &Path) -> SendHistory {
        SendHistory {
            store: Store::Dir(state_dir.join("sends")),
        }
    }

    pub fn in_memory() -> SendHistory {
        SendHistory {
            store: Store::Memory(Mutex::default()),
        }
    }

    /// Writes the send, then its summary, then deletes sends beyond `KEEP_SENDS`.
    pub fn save(&self, record: &SendRecord) -> Result<(), HistoryError> {
        match &self.store {
            Store::Dir(dir) => {
                std::fs::create_dir_all(dir)?;
                let id = record.header.id.as_str();
                let send = serde_json::to_vec(record).map_err(std::io::Error::other)?;
                let meta =
                    serde_json::to_vec(&SendMeta::of(record)).map_err(std::io::Error::other)?;
                write(&dir.join(format!("{id}{SEND_EXT}")), &send)?;
                write(&dir.join(format!("{id}{META_EXT}")), &meta)?;
                let ids = run_ids(dir);
                let extra = ids.len().saturating_sub(KEEP_SENDS);
                for old in &ids[..extra] {
                    // Retried by the next save if another process holds a file open.
                    if let Err(e) = remove(dir, old) {
                        tracing::warn!("cannot prune send history in {}: {e}", dir.display());
                    }
                }
            }
            Store::Memory(sends) => {
                let mut sends = sends.lock().unwrap_or_else(|e| e.into_inner());
                sends.insert(record.header.id.clone(), record.clone());
                while sends.len() > KEEP_SENDS {
                    sends.pop_first();
                }
            }
        }
        Ok(())
    }

    /// Newest first, of one collection or all; unreadable or unknown-version entries are
    /// skipped.
    pub fn list(&self, collection: Option<&CollectionId>) -> Vec<SendMeta> {
        let of = |root: &str| collection.is_none_or(|c| CollectionId::of_canonical(root) == *c);
        match &self.store {
            Store::Dir(dir) => run_ids(dir)
                .into_iter()
                .rev()
                .filter_map(|id| {
                    let bytes = std::fs::read(dir.join(format!("{id}{META_EXT}"))).ok()?;
                    let meta: SendMeta = serde_json::from_slice(&bytes).ok()?;
                    (meta.version == SEND_RECORD_VERSION && meta.header.id == id).then_some(meta)
                })
                .filter(|m| of(&m.header.collection_root))
                .collect(),
            Store::Memory(sends) => sends
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .rev()
                .filter(|r| of(&r.header.collection_root))
                .map(SendMeta::of)
                .collect(),
        }
    }

    pub fn load(&self, id: &RunId) -> Result<SendRecord, HistoryError> {
        match &self.store {
            Store::Dir(dir) => {
                let bytes = match std::fs::read(dir.join(format!("{id}{SEND_EXT}"))) {
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
                let record: SendRecord =
                    serde_json::from_slice(&bytes).map_err(|e| unreadable(e.to_string()))?;
                if record.version != SEND_RECORD_VERSION {
                    return Err(unreadable(format!(
                        "unsupported version {}",
                        record.version
                    )));
                }
                Ok(record)
            }
            Store::Memory(sends) => sends
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .cloned()
                .ok_or_else(|| HistoryError::NotFound(id.to_string())),
        }
    }

    /// Deletes the sends of one collection, or all; returns how many.
    pub fn clear(&self, collection: Option<&CollectionId>) -> Result<usize, HistoryError> {
        match &self.store {
            Store::Dir(dir) => {
                let ids: Vec<RunId> = match collection {
                    None => run_ids(dir),
                    Some(_) => self
                        .list(collection)
                        .into_iter()
                        .map(|m| m.header.id)
                        .collect(),
                };
                for id in &ids {
                    remove(dir, id)?;
                }
                Ok(ids.len())
            }
            Store::Memory(sends) => {
                let mut sends = sends.lock().unwrap_or_else(|e| e.into_inner());
                let before = sends.len();
                sends.retain(|_, r| {
                    collection.is_some_and(|c| {
                        CollectionId::of_canonical(&r.header.collection_root) != *c
                    })
                });
                Ok(before - sends.len())
            }
        }
    }
}
