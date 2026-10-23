//! Reads and writes request and environment files inside a collection root,
//! with content-hash conflict detection and atomic replacement.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use sha2::{Digest, Sha256};

use crate::bru::model::{EnvironmentFile, RequestFile};
use crate::request::{HttpRequest, KeyValue};

/// Lowercase hex SHA-256 of a file's bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContentHash(pub String);

impl ContentHash {
    pub fn of(bytes: &[u8]) -> ContentHash {
        let digest = Sha256::digest(bytes);
        ContentHash(digest.iter().map(|b| format!("{b:02x}")).collect())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stored<T> {
    pub value: T,
    pub hash: ContentHash,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Parse(String),
    #[error("file changed on disk")]
    Conflict { current: Stored<String> },
    #[error("`{0}` is not a path inside the collection")]
    OutsideRoot(String),
    #[error("`{0}` not found")]
    NotFound(String),
}

/// Maps a `/`-separated item ID relative to `root` to a path under `root`.
///
/// The returned path is `root` joined with the ID's components (not
/// canonicalized), so it matches the paths stored in a loaded `Collection`.
/// Rejects absolute IDs, empty, `.` and `..` components, and any ID whose
/// existing part resolves (through symlinks) outside the canonical root.
pub fn resolve_item_path(root: &Path, item_id: &str) -> Result<PathBuf, StoreError> {
    let outside = || StoreError::OutsideRoot(item_id.to_owned());
    if item_id.is_empty() || item_id.contains('\\') || item_id.contains('\0') {
        return Err(outside());
    }
    let mut path = root.to_path_buf();
    for part in item_id.split('/') {
        let mut components = Path::new(part).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) if name == part => path.push(name),
            _ => return Err(outside()),
        }
    }
    let canonical_root = root.canonicalize()?;
    let mut existing = path.as_path();
    while !existing.exists() {
        existing = existing.parent().ok_or_else(outside)?;
    }
    if !existing.canonicalize()?.starts_with(&canonical_root) {
        return Err(outside());
    }
    Ok(path)
}

fn read_existing(path: &Path, item_id: &str) -> Result<Stored<String>, StoreError> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let hash = ContentHash::of(&bytes);
            let value = String::from_utf8(bytes)
                .map_err(|_| StoreError::Parse(format!("`{item_id}` is not valid UTF-8")))?;
            Ok(Stored { value, hash })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(StoreError::NotFound(item_id.to_owned()))
        }
        Err(e) => Err(e.into()),
    }
}

pub fn read_request(root: &Path, item_id: &str) -> Result<Stored<HttpRequest>, StoreError> {
    let path = resolve_item_path(root, item_id)?;
    let text = read_existing(&path, item_id)?;
    let file = RequestFile::parse(&text.value).map_err(|e| StoreError::Parse(e.to_string()))?;
    Ok(Stored {
        value: file.request,
        hash: text.hash,
    })
}

/// Writes `request` into the file at `item_id`, touching only changed blocks.
/// Fails with `Conflict` (and writes nothing) when the file no longer hashes to `expected`.
///
/// Returns the request as parsed back from the written text, which can differ
/// from `request` where `.bru` cannot represent an input (e.g. values are trimmed).
pub fn save_request(
    root: &Path,
    item_id: &str,
    request: &HttpRequest,
    expected: &ContentHash,
) -> Result<Stored<HttpRequest>, StoreError> {
    let path = resolve_item_path(root, item_id)?;
    let lock = lock_file(&path).map_err(|e| not_found_as(e, item_id))?;
    let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
    let current = check_current(&path, item_id, expected)?;
    let mut file =
        RequestFile::parse(&current.value).map_err(|e| StoreError::Parse(e.to_string()))?;
    file.request = request.clone();
    let text = file.to_bru();
    let reread = RequestFile::parse(&text).map_err(|e| StoreError::Parse(e.to_string()))?;
    let hash = write_atomic(&path, text.as_bytes())?;
    Ok(Stored {
        value: reread.request,
        hash,
    })
}

/// Replaces the `vars` of `environments/<name>.bru`. Secret names and other blocks are kept.
pub fn save_environment(
    root: &Path,
    name: &str,
    vars: &[KeyValue],
    expected: &ContentHash,
) -> Result<ContentHash, StoreError> {
    if name.contains('/') {
        return Err(StoreError::OutsideRoot(name.to_owned()));
    }
    let item_id = format!("environments/{name}.bru");
    let path =
        resolve_item_path(root, &item_id).map_err(|_| StoreError::OutsideRoot(name.to_owned()))?;
    let lock = lock_file(&path).map_err(|e| not_found_as(e, &item_id))?;
    let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
    let current = check_current(&path, &item_id, expected)?;
    let mut file =
        EnvironmentFile::parse(&current.value).map_err(|e| StoreError::Parse(e.to_string()))?;
    file.vars = vars.to_vec();
    write_atomic(&path, file.to_bru().as_bytes())
}

/// Rewrites `path` (inside the collection) under its lock: `update` gets the current text
/// (`None` when the file does not exist) and returns the new text. Writes only when the text
/// changed; `Ok(true)` when it did.
pub fn update_file(
    path: &Path,
    update: impl FnOnce(Option<&str>) -> Result<String, String>,
) -> Result<bool, StoreError> {
    let lock = lock_file(path).or_else(|_| lock_missing(path))?;
    let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
    let current = match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let next = update(current.as_deref()).map_err(StoreError::Parse)?;
    if current.as_deref() == Some(next.as_str()) {
        return Ok(false);
    }
    write_atomic(path, next.as_bytes())?;
    Ok(true)
}

/// The lock for a file that does not exist yet, keyed by its canonical directory.
fn lock_missing(path: &Path) -> Result<Arc<Mutex<()>>, StoreError> {
    let dir = path
        .parent()
        .ok_or_else(|| StoreError::NotFound(path.display().to_string()))?;
    let name = path
        .file_name()
        .ok_or_else(|| StoreError::NotFound(path.display().to_string()))?;
    lock_key(dir.canonicalize()?.join(name))
}

/// One lock per file (by canonical path), held from the hash check to the
/// rename so two saves with the same base hash cannot both succeed.
/// Guards writers in this process; other programs editing the file are
/// caught by the hash check on the next save.
fn lock_file(path: &Path) -> Result<Arc<Mutex<()>>, StoreError> {
    lock_key(path.canonicalize()?)
}

fn lock_key(key: PathBuf) -> Result<Arc<Mutex<()>>, StoreError> {
    static LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = LazyLock::new(Mutex::default);
    let mut locks = LOCKS.lock().unwrap_or_else(PoisonError::into_inner);
    Ok(locks.entry(key).or_default().clone())
}

fn check_current(
    path: &Path,
    item_id: &str,
    expected: &ContentHash,
) -> Result<Stored<String>, StoreError> {
    let current = read_existing(path, item_id)?;
    if &current.hash != expected {
        return Err(StoreError::Conflict { current });
    }
    Ok(current)
}

fn not_found_as(e: StoreError, item_id: &str) -> StoreError {
    match e {
        StoreError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
            StoreError::NotFound(item_id.to_owned())
        }
        e => e,
    }
}

/// Writes to a temp file in the same directory, then renames it over `path`,
/// keeping the original file's permissions.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<ContentHash, StoreError> {
    let dir = path
        .parent()
        .ok_or_else(|| StoreError::NotFound(path.display().to_string()))?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".plover-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    if let Ok(meta) = std::fs::metadata(path) {
        tmp.as_file().set_permissions(meta.permissions())?;
    }
    tmp.persist(path).map_err(|e| StoreError::Io(e.error))?;
    Ok(ContentHash::of(bytes))
}
