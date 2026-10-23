//! Where secret environment values (`vars:secret`) live: the OS keychain through
//! `keyring`, with a per-secret memory overlay for values the keychain refuses, or
//! memory only when no keychain is available at startup. Values never touch disk and
//! are never logged.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use keyring::Entry;
use keyring::credential::CredentialBuilder;

use crate::workspace::CollectionId;

/// Keychain service name for Plover's secrets (decision P4-3).
pub const KEYCHAIN_SERVICE: &str = "dev.plover";

/// One secret: the collection it belongs to, its environment and its variable name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretKey {
    pub collection: CollectionId,
    pub env: String,
    pub name: String,
}

impl SecretKey {
    /// The keychain account: `<collection id>/<env>/<name>`.
    pub fn account(&self) -> String {
        format!("{}/{}/{}", self.collection.0, self.env, self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretBackend {
    Keychain,
    Memory,
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// No usable keychain (no Secret Service, no keychain file).
    #[error("keychain unavailable: {0}")]
    Unavailable(String),
    /// The keychain refused this item (user cancelled, locked, access denied).
    #[error("keychain refused: {0}")]
    Denied(String),
    /// The value is longer than the platform store accepts.
    #[error("value too long for the keychain: {0}")]
    TooLong(String),
    #[error("keychain error: {0}")]
    Failed(String),
}

/// A raw keychain: stores exactly what it is given, or fails.
pub trait Keychain: Send + Sync {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, SecretError>;
    fn set(&self, key: &SecretKey, value: &str) -> Result<(), SecretError>;
    /// Deleting a secret that isn't stored is not an error.
    fn delete(&self, key: &SecretKey) -> Result<(), SecretError>;
}

/// Header value of a secret stored in chunks: `plover:chunks:v1:<count>`. A value that
/// itself starts with this prefix is always chunked, so a header is never ambiguous.
const CHUNK_MARKER: &str = "plover:chunks:v1:";
/// UTF-16 units per chunk; Windows Credential Manager stores at most 1280.
const CHUNK_UNITS: usize = 1000;

/// The OS keychain: macOS Keychain, Windows Credential Manager, or the Secret Service on
/// Linux. Values too long for one entry are split into `<account>#0..n` entries behind a
/// header entry at `<account>`.
pub struct KeyringStore {
    service: String,
    /// `None` uses keyring's platform default.
    builder: Option<Box<CredentialBuilder>>,
}

impl KeyringStore {
    pub fn new(service: &str) -> KeyringStore {
        KeyringStore {
            service: service.to_owned(),
            builder: None,
        }
    }

    /// A store backed by a specific keyring credential builder (tests, other backends).
    pub fn with_builder(service: &str, builder: Box<CredentialBuilder>) -> KeyringStore {
        KeyringStore {
            service: service.to_owned(),
            builder: Some(builder),
        }
    }

    fn entry(&self, account: &str) -> Result<Entry, SecretError> {
        match &self.builder {
            Some(builder) => builder
                .build(None, &self.service, account)
                .map(Entry::new_with_credential),
            None => Entry::new(&self.service, account),
        }
        .map_err(map_error)
    }

    fn read(&self, account: &str) -> Result<Option<String>, SecretError> {
        match self.entry(account)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_error(e)),
        }
    }

    fn write(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.entry(account)?.set_password(value).map_err(map_error)
    }

    fn remove(&self, account: &str) -> Result<(), SecretError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_error(e)),
        }
    }

    /// How many chunks `account` currently has (0 when it is a plain value or absent).
    fn chunk_count(&self, account: &str) -> Result<usize, SecretError> {
        Ok(self
            .read(account)?
            .and_then(|header| header.strip_prefix(CHUNK_MARKER)?.parse().ok())
            .unwrap_or(0))
    }

    fn remove_chunks(&self, account: &str, count: usize) -> Result<(), SecretError> {
        (0..count).try_for_each(|i| self.remove(&format!("{account}#{i}")))
    }

    fn write_chunks(&self, account: &str, value: &str) -> Result<(), SecretError> {
        let chunks = split_utf16(value, CHUNK_UNITS);
        for (i, chunk) in chunks.iter().enumerate() {
            self.write(&format!("{account}#{i}"), chunk)?;
        }
        self.write(account, &format!("{CHUNK_MARKER}{}", chunks.len()))
    }
}

/// Splits `value` into pieces of at most `units` UTF-16 code units, on char boundaries.
fn split_utf16(value: &str, units: usize) -> Vec<String> {
    let mut chunks = vec![String::new()];
    let mut used = 0;
    for c in value.chars() {
        if used + c.len_utf16() > units {
            chunks.push(String::new());
            used = 0;
        }
        used += c.len_utf16();
        if let Some(last) = chunks.last_mut() {
            last.push(c);
        }
    }
    chunks
}

fn map_error(err: keyring::Error) -> SecretError {
    match err {
        keyring::Error::NoStorageAccess(e) => SecretError::Unavailable(e.to_string()),
        keyring::Error::PlatformFailure(e) => SecretError::Denied(e.to_string()),
        keyring::Error::TooLong(field, max) => SecretError::TooLong(format!("{field} (max {max})")),
        other => SecretError::Failed(other.to_string()),
    }
}

impl Keychain for KeyringStore {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, SecretError> {
        let account = key.account();
        let Some(value) = self.read(&account)? else {
            return Ok(None);
        };
        let Some(count) = value.strip_prefix(CHUNK_MARKER) else {
            return Ok(Some(value));
        };
        let count: usize = count
            .parse()
            .map_err(|_| SecretError::Failed("unreadable chunk header".into()))?;
        let mut whole = String::new();
        for i in 0..count {
            match self.read(&format!("{account}#{i}"))? {
                Some(part) => whole.push_str(&part),
                None => return Err(SecretError::Failed("incomplete chunked value".into())),
            }
        }
        Ok(Some(whole))
    }

    fn set(&self, key: &SecretKey, value: &str) -> Result<(), SecretError> {
        let account = key.account();
        let old_chunks = self.chunk_count(&account)?;
        let single = if value.starts_with(CHUNK_MARKER) {
            Err(SecretError::TooLong(
                "value looks like a chunk header".into(),
            ))
        } else {
            self.write(&account, value)
        };
        match single {
            Ok(()) => self.remove_chunks(&account, old_chunks),
            Err(SecretError::TooLong(_)) => {
                let written = self.write_chunks(&account, value);
                if written.is_err() {
                    // Leave nothing half-written; the caller keeps the value in memory.
                    let count = old_chunks.max(split_utf16(value, CHUNK_UNITS).len());
                    let _ = self.remove_chunks(&account, count);
                    let _ = self.remove(&account);
                    return written;
                }
                let new_chunks = split_utf16(value, CHUNK_UNITS).len();
                (new_chunks..old_chunks).try_for_each(|i| self.remove(&format!("{account}#{i}")))
            }
            Err(e) => Err(e),
        }
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        let account = key.account();
        let chunks = self.chunk_count(&account)?;
        self.remove_chunks(&account, chunks)?;
        self.remove(&account)
    }
}

/// A secret's value and where it is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSecret {
    pub value: String,
    pub backend: SecretBackend,
}

/// What happened to a value that was set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetOutcome {
    pub backend: SecretBackend,
    /// Why it is in memory instead of the keychain (no value included).
    pub warning: Option<String>,
}

/// Secrets for one process: the keychain when available, plus memory for values the
/// keychain refused (per secret) or for everything when there is no keychain.
pub struct SecretVault {
    keychain: Option<Box<dyn Keychain>>,
    /// Decided once at startup: whether new values go to the keychain at all.
    available: bool,
    memory: Mutex<HashMap<SecretKey, String>>,
}

impl SecretVault {
    /// No keychain: every secret lives in memory (web tests, embedding).
    pub fn memory() -> SecretVault {
        SecretVault {
            keychain: None,
            available: false,
            memory: Mutex::default(),
        }
    }

    /// Uses `keychain` if a probe read succeeds; otherwise memory only, said up front.
    pub fn with_keychain(keychain: Box<dyn Keychain>) -> SecretVault {
        let probe = SecretKey {
            collection: CollectionId("plover-probe".into()),
            env: String::new(),
            name: String::new(),
        };
        let available = match keychain.get(&probe) {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!(error = %e, "OS keychain unavailable; secrets stay in memory only");
                false
            }
        };
        SecretVault {
            keychain: available.then_some(keychain),
            available,
            memory: Mutex::default(),
        }
    }

    /// Where new values go: the keychain, or memory when it was unavailable at startup.
    pub fn availability(&self) -> SecretBackend {
        if self.available {
            SecretBackend::Keychain
        } else {
            SecretBackend::Memory
        }
    }

    fn overlay(&self) -> std::sync::MutexGuard<'_, HashMap<SecretKey, String>> {
        self.memory.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The memory overlay first, then the keychain.
    pub fn get(&self, key: &SecretKey) -> Result<Option<StoredSecret>, SecretError> {
        if let Some(value) = self.overlay().get(key) {
            return Ok(Some(StoredSecret {
                value: value.clone(),
                backend: SecretBackend::Memory,
            }));
        }
        match &self.keychain {
            Some(keychain) => Ok(keychain.get(key)?.map(|value| StoredSecret {
                value,
                backend: SecretBackend::Keychain,
            })),
            None => Ok(None),
        }
    }

    /// Stores in the keychain; if it refuses, keeps this secret in memory and says why.
    pub fn set(&self, key: &SecretKey, value: &str) -> Result<SetOutcome, SecretError> {
        let warning = match &self.keychain {
            Some(keychain) => match keychain.set(key, value) {
                Ok(()) => {
                    self.overlay().remove(key);
                    return Ok(SetOutcome {
                        backend: SecretBackend::Keychain,
                        warning: None,
                    });
                }
                Err(e) => {
                    tracing::warn!(name = %key.name, error = %e, "keychain refused a secret; keeping it in memory");
                    format!("{}: {e}; kept in memory until Plover quits", key.name)
                }
            },
            None => format!(
                "{}: OS keychain unavailable; kept in memory until Plover quits",
                key.name
            ),
        };
        self.overlay().insert(key.clone(), value.to_owned());
        Ok(SetOutcome {
            backend: SecretBackend::Memory,
            warning: Some(warning),
        })
    }

    /// Removes the secret from memory and from the keychain; a keychain failure is
    /// returned (the memory value is gone either way).
    pub fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        self.overlay().remove(key);
        match &self.keychain {
            Some(keychain) => keychain.delete(key),
            None => Ok(()),
        }
    }
}

/// The OS keychain if available, else memory. `PLOVER_KEYCHAIN_SERVICE` overrides the
/// service name (for manual tests and CI that must not touch real entries).
pub fn default_store() -> std::sync::Arc<SecretVault> {
    let service =
        std::env::var("PLOVER_KEYCHAIN_SERVICE").unwrap_or_else(|_| KEYCHAIN_SERVICE.to_owned());
    std::sync::Arc::new(SecretVault::with_keychain(Box::new(KeyringStore::new(
        &service,
    ))))
}

/// The stored values of the secrets environment `env` declares (`vars:secret`), for
/// `plv run --use-keychain`. A missing or unreadable value is skipped (logged without it).
pub fn env_secrets(
    store: &SecretVault,
    collection: &crate::collection::Collection,
    env: &str,
) -> crate::vars::VarMap {
    let mut values = crate::vars::VarMap::new();
    let Some(environment) = collection.environment(env) else {
        return values;
    };
    let Ok(id) = CollectionId::for_root(&collection.root) else {
        return values;
    };
    for secret in &environment.file.secrets {
        let key = SecretKey {
            collection: id.clone(),
            env: env.to_owned(),
            name: secret.name.clone(),
        };
        match store.get(&key) {
            Ok(Some(stored)) => {
                values.insert(secret.name.clone(), stored.value);
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(name = %secret.name, error = %e, "could not read secret"),
        }
    }
    values
}
