//! Secret values live in the OS keychain (keyring), falling back to memory when the
//! keychain is unavailable; they never touch disk.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
use plover_core::executor::ExecOptions;
use plover_core::secrets::{
    FallbackStore, KeyringStore, MemoryStore, SecretBackend, SecretKey, SecretStore,
};
use plover_core::workspace::{CollectionId, Workspace};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

type Shared = Arc<Mutex<HashMap<(String, String), Vec<u8>>>>;

/// A keyring backend that persists in a shared map (keyring's own mock does not persist
/// across entries), or fails every call like an unavailable platform store.
struct TestBuilder {
    store: Shared,
    fail: bool,
}

struct TestCredential {
    store: Shared,
    key: (String, String),
    fail: bool,
}

impl CredentialApi for TestCredential {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        if self.fail {
            return Err(keyring::Error::PlatformFailure("denied".into()));
        }
        self.store
            .lock()
            .unwrap()
            .insert(self.key.clone(), secret.to_vec());
        Ok(())
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        if self.fail {
            return Err(keyring::Error::NoStorageAccess("locked".into()));
        }
        self.store
            .lock()
            .unwrap()
            .get(&self.key)
            .cloned()
            .ok_or(keyring::Error::NoEntry)
    }
    fn delete_credential(&self) -> keyring::Result<()> {
        if self.fail {
            return Err(keyring::Error::PlatformFailure("denied".into()));
        }
        self.store
            .lock()
            .unwrap()
            .remove(&self.key)
            .map(|_| ())
            .ok_or(keyring::Error::NoEntry)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl CredentialBuilderApi for TestBuilder {
    fn build(
        &self,
        _target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<Credential>> {
        Ok(Box::new(TestCredential {
            store: self.store.clone(),
            key: (service.to_owned(), user.to_owned()),
            fail: self.fail,
        }))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn keyring_store(fail: bool) -> (KeyringStore, Shared) {
    let shared = Shared::default();
    let store = KeyringStore::with_builder(
        "dev.plover.test",
        Box::new(TestBuilder {
            store: shared.clone(),
            fail,
        }),
    );
    (store, shared)
}

fn key(collection: &str, env: &str, name: &str) -> SecretKey {
    SecretKey {
        collection: CollectionId(collection.to_owned()),
        env: env.to_owned(),
        name: name.to_owned(),
    }
}

#[test]
fn set_get_delete_roundtrip() {
    let (store, shared) = keyring_store(false);
    let k = key("abc", "local", "token");
    assert_eq!(store.get(&k).unwrap(), None);
    store.set(&k, "s3cret").unwrap();
    assert_eq!(store.get(&k).unwrap().as_deref(), Some("s3cret"));
    assert_eq!(store.kind(), SecretBackend::Keychain);
    // P4-3: service dev.plover…, account <collection>/<env>/<name>.
    assert!(
        shared
            .lock()
            .unwrap()
            .contains_key(&("dev.plover.test".to_owned(), "abc/local/token".to_owned()))
    );
    store.delete(&k).unwrap();
    assert_eq!(store.get(&k).unwrap(), None);
    store.delete(&k).expect("deleting a missing entry is fine");
}

#[test]
fn keys_isolated_per_collection_and_env() {
    let (store, _) = keyring_store(false);
    store.set(&key("c1", "local", "t"), "a").unwrap();
    store.set(&key("c1", "prod", "t"), "b").unwrap();
    store.set(&key("c2", "local", "t"), "c").unwrap();
    assert_eq!(
        store.get(&key("c1", "local", "t")).unwrap().as_deref(),
        Some("a")
    );
    assert_eq!(
        store.get(&key("c1", "prod", "t")).unwrap().as_deref(),
        Some("b")
    );
    assert_eq!(
        store.get(&key("c2", "local", "t")).unwrap().as_deref(),
        Some("c")
    );
}

#[test]
fn keychain_failure_falls_back_to_memory() {
    let (failing, shared) = keyring_store(true);
    let store = FallbackStore::new(Box::new(failing));
    let k = key("abc", "local", "token");
    store.set(&k, "v1").expect("falls back instead of failing");
    assert_eq!(store.get(&k).unwrap().as_deref(), Some("v1"));
    assert_eq!(store.kind(), SecretBackend::Memory);
    store.delete(&k).unwrap();
    assert_eq!(store.get(&k).unwrap(), None);
    assert!(
        shared.lock().unwrap().is_empty(),
        "nothing reached the keychain"
    );
}

#[test]
fn fallback_uses_keychain_while_it_works() {
    let (ok, shared) = keyring_store(false);
    let store = FallbackStore::new(Box::new(ok));
    store.set(&key("a", "e", "n"), "v").unwrap();
    assert_eq!(store.kind(), SecretBackend::Keychain);
    assert_eq!(shared.lock().unwrap().len(), 1);
}

fn collection_with_secret_env() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("bruno.json"),
        r#"{"version":"1","name":"s","type":"collection"}"#,
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("environments")).unwrap();
    std::fs::write(
        dir.path().join("environments/local.bru"),
        "vars {\n  other: 1\n}\nvars:secret [\n  host,\n  token\n]\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("ping.bru"),
        "meta {\n  name: ping\n  type: http\n  seq: 1\n}\n\nget {\n  url: {{host}}/ping\n  body: none\n  auth: none\n}\n",
    )
    .unwrap();
    dir
}

#[test]
fn secrets_set_lists_names_only() {
    let dir = collection_with_secret_env();
    let ws = Workspace::with_secrets(ExecOptions::default(), Arc::new(MemoryStore::default()));
    let id = ws.open(dir.path()).unwrap();
    let open = ws.get(&id).unwrap();
    open.update_secrets("local", [("token".to_owned(), Some("t".to_owned()))].into())
        .unwrap();
    assert_eq!(open.secret_names_set("local"), ["token"]);
    open.update_secrets("local", [("token".to_owned(), None)].into())
        .unwrap();
    assert!(open.secret_names_set("local").is_empty());
}

#[tokio::test]
async fn send_uses_stored_secret() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ping"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let dir = collection_with_secret_env();
    let (keychain, _) = keyring_store(false);
    let store: Arc<dyn SecretStore> = Arc::new(keychain);
    let ws = Workspace::with_secrets(ExecOptions::default(), store.clone());
    let id = ws.open(dir.path()).unwrap();
    // Stored earlier (e.g. by a previous run of the app): read from the store, not memory.
    store
        .set(&key(&id.0, "local", "host"), &server.uri())
        .unwrap();
    let open = ws.get(&id).unwrap();
    assert_eq!(open.secret_names_set("local"), ["host"]);
    let outcome = plover_core::runner::send_one(
        &open,
        plover_core::runner::SendInput {
            item_id: "ping.bru",
            draft: None,
            env: Some("local"),
        },
    )
    .await
    .unwrap();
    assert_eq!(outcome.response.unwrap().status, 200);
}

#[test]
fn env_secrets_for_cli_read_declared_names() {
    let dir = collection_with_secret_env();
    let collection = plover_core::collection::Collection::load(dir.path()).unwrap();
    let (keychain, _) = keyring_store(false);
    let id = CollectionId::for_root(dir.path()).unwrap();
    keychain.set(&key(&id.0, "local", "token"), "kc").unwrap();
    keychain
        .set(&key(&id.0, "local", "undeclared"), "x")
        .unwrap();
    let values = plover_core::secrets::env_secrets(&keychain, &collection, "local");
    assert_eq!(values, [("token".to_owned(), "kc".to_owned())].into());
}
