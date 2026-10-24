//! Reads and writes request and environment files inside a collection root,
//! with content-hash conflict detection and atomic replacement.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use sha2::{Digest, Sha256};

use crate::bru::model::{EnvironmentFile, RequestFile, SecretVar};
use crate::bru::syntax::BruDocument;
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
    #[error("`{0}` already exists")]
    Exists(String),
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
    let lock = lock_file(path)?;
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

/// One lock per file, held from the hash check to the rename so two saves with the same
/// base hash cannot both succeed. Guards writers in this process; other programs editing
/// the file are caught by the hash check on the next save.
///
/// The key is the canonical directory plus the file name, so taking the lock never opens
/// the file itself: on Windows, canonicalizing a file that another thread is renaming over
/// fails with a sharing violation. Windows names are case-insensitive, so the name is
/// lower-cased there.
fn lock_file(path: &Path) -> Result<Arc<Mutex<()>>, StoreError> {
    let dir = path
        .parent()
        .ok_or_else(|| StoreError::NotFound(path.display().to_string()))?;
    let name = path
        .file_name()
        .ok_or_else(|| StoreError::NotFound(path.display().to_string()))?;
    let name = if cfg!(windows) {
        std::ffi::OsString::from(name.to_string_lossy().to_lowercase())
    } else {
        name.to_os_string()
    };
    lock_key(dir.canonicalize()?.join(name))
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
    // Windows refuses to replace a file that another program (antivirus, an editor, a
    // reader in this process) has open without delete sharing; that clears up quickly.
    let mut attempt = 0;
    loop {
        match tmp.persist(path) {
            Ok(_) => break,
            Err(e)
                if cfg!(windows)
                    && attempt < 10
                    && e.error.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                attempt += 1;
                tmp = e.file;
                std::thread::sleep(std::time::Duration::from_millis(20 * attempt));
            }
            Err(e) => return Err(StoreError::Io(e.error)),
        }
    }
    Ok(ContentHash::of(bytes))
}

// ---- creating files

/// File and folder names are cut to this many bytes (Plover's call; Bruno allows 255
/// characters, which can be more bytes than some file systems take).
const MAX_NAME_BYTES: usize = 200;

/// Bruno's `sanitizeName` (`@usebruno/common/utils/naming.ts`): characters that are not
/// allowed in file names become `-`, leading spaces and hyphens and trailing dots and
/// spaces are trimmed. Also cuts the name to 200 bytes and appends `-` to Windows device
/// names (`CON`, `LPT1`, ...), which Bruno's `validateName` rejects.
pub fn sanitize_name(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c < ' ' {
                '-'
            } else {
                c
            }
        })
        .collect();
    let trim_end = |s: &str| {
        s.trim_end_matches(|c: char| c == '.' || c.is_whitespace())
            .to_owned()
    };
    let mut out = trim_end(replaced.trim_start_matches(|c: char| c == '-' || c.is_whitespace()));
    if out.len() > MAX_NAME_BYTES {
        let mut cut = MAX_NAME_BYTES;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out = trim_end(&out);
    }
    if is_reserved_device_name(&out) {
        out.push('-');
    }
    out
}

fn is_reserved_device_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit())
}

/// `name`, `name 2`, `name 3`, ...: the first whose sanitized file stem `is_taken` says is
/// free. `is_taken` gets the stem lower-cased, because collection folders often live on
/// case-insensitive file systems.
pub fn unique_name(name: &str, is_taken: impl Fn(&str) -> bool) -> String {
    let free = |candidate: &str| !is_taken(&sanitize_name(candidate).to_lowercase());
    if free(name) {
        return name.to_owned();
    }
    (2..)
        .map(|n| format!("{name} {n}"))
        .find(|c| free(c))
        .unwrap_or_else(|| name.to_owned())
}

/// [`unique_name`] against the `.bru` files in `dir`.
pub fn unique_file_name(dir: &Path, name: &str) -> String {
    let taken = taken_names(dir);
    unique_name(name, |stem| taken.contains(&format!("{stem}.bru")))
}

/// Lower-cased names of the entries in `dir`.
fn taken_names(dir: &Path) -> std::collections::BTreeSet<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().to_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

fn join_id(folder_id: &str, name: &str) -> String {
    if folder_id.is_empty() {
        name.to_owned()
    } else {
        format!("{folder_id}/{name}")
    }
}

/// The directory of folder `folder_id` (`""` is the collection root).
fn folder_dir(root: &Path, folder_id: &str) -> Result<PathBuf, StoreError> {
    let dir = if folder_id.is_empty() {
        root.to_path_buf()
    } else {
        resolve_item_path(root, folder_id)?
    };
    if !dir.is_dir() {
        return Err(StoreError::NotFound(folder_id.to_owned()));
    }
    Ok(dir)
}

/// The next `seq` after the requests directly in `dir`: the highest plus one, or 1.
pub fn next_seq(dir: &Path) -> f64 {
    let max = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.ends_with(".bru") && name != "folder.bru" && name != "collection.bru"
        })
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|text| RequestFile::parse(&text).ok())
        .map(|f| f.request.seq)
        .fold(0.0_f64, f64::max);
    max.floor() + 1.0
}

/// Writes a new request file `<sanitize_name(name)>.bru` in folder `folder_id` (`""` is
/// the root) and returns its item ID with the request as read back. The request's
/// `name` becomes `name`, and a `seq` of 0 becomes the folder's next seq. Fails with
/// `Exists` (writing nothing) when a file or folder of that name is there.
pub fn create_request(
    root: &Path,
    folder_id: &str,
    name: &str,
    request: &HttpRequest,
) -> Result<(String, Stored<HttpRequest>), StoreError> {
    let dir = folder_dir(root, folder_id)?;
    let name = name.trim();
    let stem = sanitize_name(name);
    if stem.is_empty() {
        return Err(StoreError::Parse("a request needs a name".into()));
    }
    let file_name = format!("{stem}.bru");
    let item_id = join_id(folder_id, &file_name);
    let path = dir.join(&file_name);
    let lock = lock_file(&path)?;
    let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
    if taken_names(&dir).contains(&file_name.to_lowercase()) {
        return Err(StoreError::Exists(item_id));
    }
    let mut request = request.clone();
    request.name = name.to_owned();
    if request.seq == 0.0 {
        request.seq = next_seq(&dir);
    }
    let text = RequestFile {
        doc: BruDocument::new(),
        request,
    }
    .to_bru();
    let reread = RequestFile::parse(&text).map_err(|e| StoreError::Parse(e.to_string()))?;
    let hash = write_new(&path, text.as_bytes()).map_err(|e| match e {
        StoreError::Io(io) if io.kind() == std::io::ErrorKind::AlreadyExists => {
            StoreError::Exists(item_id.clone())
        }
        e => e,
    })?;
    Ok((
        item_id,
        Stored {
            value: reread.request,
            hash,
        },
    ))
}

/// Creates folder `<sanitize_name(name)>` in folder `parent_id` (`""` is the root) and
/// returns its ID. No `folder.bru` is written.
pub fn create_folder(root: &Path, parent_id: &str, name: &str) -> Result<String, StoreError> {
    let dir = folder_dir(root, parent_id)?;
    let stem = sanitize_name(name.trim());
    if stem.is_empty() {
        return Err(StoreError::Parse("a folder needs a name".into()));
    }
    let id = join_id(parent_id, &stem);
    if taken_names(&dir).contains(&stem.to_lowercase()) {
        return Err(StoreError::Exists(id));
    }
    match std::fs::create_dir(dir.join(&stem)) {
        Ok(()) => Ok(id),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(StoreError::Exists(id)),
        Err(e) => Err(e.into()),
    }
}

/// Adds `names` to the `vars:secret` block of `environments/<env>.bru`, creating the file
/// (and `environments/`) when missing. Names already listed stay as they are, and nothing
/// is written when every name is there. Returns the file's hash.
pub fn add_secret_names(
    root: &Path,
    env: &str,
    names: &[String],
) -> Result<ContentHash, StoreError> {
    if env.trim().is_empty() || env.contains(['/', '\\', '\0']) || env == "." || env == ".." {
        return Err(StoreError::OutsideRoot(env.to_owned()));
    }
    let dir = root.join("environments");
    std::fs::create_dir_all(&dir)?;
    let path = resolve_item_path(root, &format!("environments/{env}.bru"))?;
    update_file(&path, |current| {
        let mut file =
            EnvironmentFile::parse(current.unwrap_or_default()).map_err(|e| e.to_string())?;
        for name in names {
            if !file.secrets.iter().any(|s| s.name == *name) {
                file.secrets.push(SecretVar::new(name.clone()));
            }
        }
        Ok(file.to_bru())
    })?;
    Ok(ContentHash::of(&std::fs::read(&path)?))
}

/// [`write_atomic`] for a file that must not exist yet: fails with `AlreadyExists`
/// instead of replacing one that appeared after the check.
fn write_new(path: &Path, bytes: &[u8]) -> Result<ContentHash, StoreError> {
    let dir = path
        .parent()
        .ok_or_else(|| StoreError::NotFound(path.display().to_string()))?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".plover-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist_noclobber(path)
        .map_err(|e| StoreError::Io(e.error))?;
    Ok(ContentHash::of(bytes))
}
