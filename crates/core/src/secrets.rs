//! Where secret environment values (`vars:secret`) live: the OS keychain through
//! `keyring`, or memory when no keychain is available. Values never touch disk
//! and are never logged.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

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
    /// The platform store can't be used (no Secret Service, locked, access denied).
    #[error("keychain unavailable: {0}")]
    Unavailable(String),
    #[error("keychain error: {0}")]
    Failed(String),
}

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, SecretError>;
    fn set(&self, key: &SecretKey, value: &str) -> Result<(), SecretError>;
    /// Deleting a secret that isn't stored is not an error.
    fn delete(&self, key: &SecretKey) -> Result<(), SecretError>;
    fn kind(&self) -> SecretBackend;
}

/// The OS keychain: macOS Keychain, Windows Credential Manager, or the Secret Service on Linux.
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

    fn entry(&self, key: &SecretKey) -> Result<Entry, SecretError> {
        let account = key.account();
        match &self.builder {
            Some(builder) => builder
                .build(None, &self.service, &account)
                .map(Entry::new_with_credential),
            None => Entry::new(&self.service, &account),
        }
        .map_err(map_error)
    }
}

fn map_error(err: keyring::Error) -> SecretError {
    match err {
        keyring::Error::PlatformFailure(e) | keyring::Error::NoStorageAccess(e) => {
            SecretError::Unavailable(e.to_string())
        }
        other => SecretError::Failed(other.to_string()),
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, SecretError> {
        match self.entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_error(e)),
        }
    }

    fn set(&self, key: &SecretKey, value: &str) -> Result<(), SecretError> {
        self.entry(key)?.set_password(value).map_err(map_error)
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_error(e)),
        }
    }

    fn kind(&self) -> SecretBackend {
        SecretBackend::Keychain
    }
}

/// Secrets for this process only; lost when it exits.
#[derive(Default)]
pub struct MemoryStore {
    values: Mutex<HashMap<SecretKey, String>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, SecretError> {
        Ok(self
            .values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned())
    }

    fn set(&self, key: &SecretKey, value: &str) -> Result<(), SecretError> {
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key);
        Ok(())
    }

    fn kind(&self) -> SecretBackend {
        SecretBackend::Memory
    }
}

/// The keychain until it reports itself unavailable, then memory for the rest of the
/// process (decision P4-4). Never falls back to disk.
pub struct FallbackStore {
    primary: Box<dyn SecretStore>,
    memory: MemoryStore,
    failed: AtomicBool,
}

impl FallbackStore {
    pub fn new(primary: Box<dyn SecretStore>) -> FallbackStore {
        FallbackStore {
            primary,
            memory: MemoryStore::default(),
            failed: AtomicBool::new(false),
        }
    }

    fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    /// Switches to memory on an `Unavailable` error; other errors are returned.
    fn degrade<T>(&self, result: Result<T, SecretError>) -> Option<Result<T, SecretError>> {
        match result {
            Err(SecretError::Unavailable(reason)) => {
                if !self.failed.swap(true, Ordering::Relaxed) {
                    tracing::warn!(%reason, "OS keychain unavailable; keeping secrets in memory only");
                }
                None
            }
            other => Some(other),
        }
    }
}

impl SecretStore for FallbackStore {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, SecretError> {
        if !self.failed()
            && let Some(result) = self.degrade(self.primary.get(key))
        {
            return result;
        }
        self.memory.get(key)
    }

    fn set(&self, key: &SecretKey, value: &str) -> Result<(), SecretError> {
        if !self.failed()
            && let Some(result) = self.degrade(self.primary.set(key, value))
        {
            return result;
        }
        self.memory.set(key, value)
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        if !self.failed()
            && let Some(result) = self.degrade(self.primary.delete(key))
        {
            return result;
        }
        self.memory.delete(key)
    }

    fn kind(&self) -> SecretBackend {
        if self.failed() {
            SecretBackend::Memory
        } else {
            self.primary.kind()
        }
    }
}

/// The OS keychain with a memory fallback. `PLOVER_KEYCHAIN_SERVICE` overrides the
/// service name (for manual tests that must not touch real entries).
pub fn default_store() -> Arc<dyn SecretStore> {
    let service =
        std::env::var("PLOVER_KEYCHAIN_SERVICE").unwrap_or_else(|_| KEYCHAIN_SERVICE.to_owned());
    Arc::new(FallbackStore::new(Box::new(KeyringStore::new(&service))))
}

/// The stored values of the secrets environment `env` declares (`vars:secret`), for
/// `plv run --use-keychain`. A missing or unreadable value is skipped (logged without it).
pub fn env_secrets(
    store: &dyn SecretStore,
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
            Ok(Some(v)) => {
                values.insert(secret.name.clone(), v);
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(name = %secret.name, error = %e, "could not read secret"),
        }
    }
    values
}
