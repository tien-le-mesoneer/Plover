//! Secret values live in the OS keychain (keyring); a value the keychain refuses is
//! kept in memory for that secret only, and a keychain that is unavailable at startup
//! makes every secret memory-only, said up front. Values never touch disk.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
use plover_core::executor::ExecOptions;
use plover_core::secrets::{KeyringStore, SecretBackend, SecretKey, SecretVault};
use plover_core::workspace::{CollectionId, Workspace};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

type Shared = Arc<Mutex<HashMap<(String, String), Vec<u8>>>>;

/// Which calls fail, and how: lets each macOS/Windows error class be simulated.
#[derive(Default, Clone)]
struct Faults {
    /// Every call fails with NoStorageAccess (no keychain service at all).
    unavailable: bool,
    /// `set` fails with PlatformFailure(message) for accounts containing this text.
    deny_set: Option<(String, String)>,
    /// `delete` fails with PlatformFailure for every account.
    deny_delete: bool,
    /// Windows-like limit on the stored value, in UTF-16 units.
    max_len: Option<usize>,
}

struct TestBuilder {
    store: Shared,
    faults: Arc<Mutex<Faults>>,
}

struct TestCredential {
    store: Shared,
    key: (String, String),
    faults: Faults,
}

impl CredentialApi for TestCredential {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        if self.faults.unavailable {
            return Err(keyring::Error::NoStorageAccess("no keychain".into()));
        }
        if let Some((needle, message)) = &self.faults.deny_set
            && self.key.1.contains(needle.as_str())
        {
            return Err(keyring::Error::PlatformFailure(message.clone().into()));
        }
        if let Some(max) = self.faults.max_len {
            let units = String::from_utf8_lossy(secret).encode_utf16().count();
            if units > max {
                return Err(keyring::Error::TooLong("password".into(), max as u32));
            }
        }
        self.store
            .lock()
            .unwrap()
            .insert(self.key.clone(), secret.to_vec());
        Ok(())
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        if self.faults.unavailable {
            return Err(keyring::Error::NoStorageAccess("no keychain".into()));
        }
        self.store
            .lock()
            .unwrap()
            .get(&self.key)
            .cloned()
            .ok_or(keyring::Error::NoEntry)
    }
    fn delete_credential(&self) -> keyring::Result<()> {
        if self.faults.unavailable {
            return Err(keyring::Error::NoStorageAccess("no keychain".into()));
        }
        if self.faults.deny_delete {
            return Err(keyring::Error::PlatformFailure("denied (-25293)".into()));
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
            faults: self.faults.lock().unwrap().clone(),
        }))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct Fake {
    store: Shared,
    faults: Arc<Mutex<Faults>>,
}

fn fake(faults: Faults) -> (Fake, KeyringStore) {
    let store = Shared::default();
    let faults = Arc::new(Mutex::new(faults));
    let keyring = KeyringStore::with_builder(
        "dev.plover.test",
        Box::new(TestBuilder {
            store: store.clone(),
            faults: faults.clone(),
        }),
    );
    (Fake { store, faults }, keyring)
}

fn vault(faults: Faults) -> (Fake, SecretVault) {
    let (fake, keyring) = fake(faults);
    (fake, SecretVault::with_keychain(Box::new(keyring)))
}

fn key(collection: &str, env: &str, name: &str) -> SecretKey {
    SecretKey {
        collection: CollectionId(collection.to_owned()),
        env: env.to_owned(),
        name: name.to_owned(),
    }
}

fn accounts(fake: &Fake) -> Vec<String> {
    let mut all: Vec<String> = fake
        .store
        .lock()
        .unwrap()
        .keys()
        .map(|k| k.1.clone())
        .collect();
    all.sort();
    all
}

#[test]
fn set_get_delete_roundtrip() {
    let (fake, vault) = vault(Faults::default());
    let k = key("abc", "local", "token");
    assert_eq!(vault.get(&k).unwrap(), None);
    let set = vault.set(&k, "s3cret").unwrap();
    assert_eq!(set.backend, SecretBackend::Keychain);
    assert_eq!(set.warning, None);
    let got = vault.get(&k).unwrap().unwrap();
    assert_eq!(
        (got.value.as_str(), got.backend),
        ("s3cret", SecretBackend::Keychain)
    );
    assert_eq!(vault.availability(), SecretBackend::Keychain);
    // P4-3: service dev.plover…, account <collection>/<env>/<name>.
    assert_eq!(accounts(&fake), ["abc/local/token"]);
    vault.delete(&k).unwrap();
    assert_eq!(vault.get(&k).unwrap(), None);
    vault.delete(&k).expect("deleting a missing entry is fine");
}

#[test]
fn keys_isolated_per_collection_and_env() {
    let (_, vault) = vault(Faults::default());
    vault.set(&key("c1", "local", "t"), "a").unwrap();
    vault.set(&key("c1", "prod", "t"), "b").unwrap();
    vault.set(&key("c2", "local", "t"), "c").unwrap();
    let value = |k| vault.get(&k).unwrap().unwrap().value;
    assert_eq!(value(key("c1", "local", "t")), "a");
    assert_eq!(value(key("c1", "prod", "t")), "b");
    assert_eq!(value(key("c2", "local", "t")), "c");
}

#[test]
fn unavailable_at_startup_uses_memory_and_says_so() {
    let (fake, vault) = vault(Faults {
        unavailable: true,
        ..Faults::default()
    });
    assert_eq!(
        vault.availability(),
        SecretBackend::Memory,
        "known before any secret is set"
    );
    let k = key("abc", "local", "token");
    let set = vault.set(&k, "v1").unwrap();
    assert_eq!(set.backend, SecretBackend::Memory);
    assert!(set.warning.unwrap().contains("unavailable"));
    assert_eq!(
        vault.get(&k).unwrap().unwrap().backend,
        SecretBackend::Memory
    );
    vault.delete(&k).unwrap();
    assert_eq!(vault.get(&k).unwrap(), None);
    assert!(accounts(&fake).is_empty());
}

#[test]
fn denied_item_falls_back_for_that_secret_only() {
    // macOS: user cancel, auth failed, interaction not allowed all arrive as PlatformFailure.
    for message in [
        "User canceled the operation. (-128)",
        "The user name or passphrase you entered is not correct. (-25293)",
        "User interaction is not allowed. (-25308)",
    ] {
        let (fake, vault) = vault(Faults {
            deny_set: Some(("/denied".into(), message.into())),
            ..Faults::default()
        });
        let ok = key("c", "local", "ok");
        let denied = key("c", "local", "denied");
        assert_eq!(
            vault.set(&ok, "a").unwrap().backend,
            SecretBackend::Keychain
        );
        let set = vault.set(&denied, "b").unwrap();
        assert_eq!(set.backend, SecretBackend::Memory, "{message}");
        assert!(set.warning.unwrap().contains(message));
        // Only that secret is affected: the keychain stays in use for everything else.
        assert_eq!(vault.availability(), SecretBackend::Keychain);
        assert_eq!(
            vault.get(&ok).unwrap().unwrap().backend,
            SecretBackend::Keychain
        );
        assert_eq!(vault.get(&denied).unwrap().unwrap().value, "b");
        assert_eq!(
            vault.set(&key("c", "local", "later"), "c").unwrap().backend,
            SecretBackend::Keychain
        );
        assert_eq!(accounts(&fake), ["c/local/later", "c/local/ok"]);
    }
}

#[test]
fn existing_keychain_entries_survive_a_failure() {
    let (fake, vault) = vault(Faults::default());
    let existing = key("c", "local", "existing");
    vault.set(&existing, "kept").unwrap();
    fake.faults.lock().unwrap().deny_set = Some(("".into(), "denied (-128)".into()));
    assert_eq!(
        vault.set(&key("c", "local", "new"), "x").unwrap().backend,
        SecretBackend::Memory
    );
    let got = vault.get(&existing).unwrap().unwrap();
    assert_eq!(
        (got.value.as_str(), got.backend),
        ("kept", SecretBackend::Keychain)
    );
}

#[test]
fn clear_deletes_from_memory_and_keychain() {
    let (fake, vault) = vault(Faults::default());
    let k = key("c", "local", "token");
    vault.set(&k, "old-in-keychain").unwrap();
    // A later update is refused, so the new value lives in memory over the old one.
    fake.faults.lock().unwrap().deny_set = Some(("".into(), "denied (-128)".into()));
    vault.set(&k, "new-in-memory").unwrap();
    assert_eq!(vault.get(&k).unwrap().unwrap().value, "new-in-memory");
    vault.delete(&k).unwrap();
    assert_eq!(
        vault.get(&k).unwrap(),
        None,
        "the old keychain value must not come back"
    );
    assert!(accounts(&fake).is_empty());
}

#[test]
fn keychain_delete_failure_is_reported() {
    let (fake, vault) = vault(Faults::default());
    let k = key("c", "local", "token");
    vault.set(&k, "v").unwrap();
    fake.faults.lock().unwrap().deny_delete = true;
    assert!(
        vault.delete(&k).is_err(),
        "the caller must learn the keychain kept it"
    );
}

#[test]
fn long_values_are_chunked() {
    let (fake, vault) = vault(Faults {
        max_len: Some(1280),
        ..Faults::default()
    });
    let k = key("c", "local", "pem");
    let long: String = "-----BEGIN PRIVATE KEY-----\n".to_owned()
        + &"MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQ\n".repeat(80)
        + "-----END PRIVATE KEY-----\n";
    assert!(long.len() > 4000);
    let set = vault.set(&k, &long).unwrap();
    assert_eq!(set.backend, SecretBackend::Keychain);
    assert!(
        accounts(&fake).len() > 2,
        "header plus chunks: {:?}",
        accounts(&fake)
    );
    assert_eq!(vault.get(&k).unwrap().unwrap().value, long);
    // A short value replaces the chunks.
    vault.set(&k, "short").unwrap();
    assert_eq!(accounts(&fake), ["c/local/pem"]);
    assert_eq!(vault.get(&k).unwrap().unwrap().value, "short");
    vault.set(&k, &long).unwrap();
    vault.delete(&k).unwrap();
    assert!(accounts(&fake).is_empty());
}

#[test]
fn chunk_failure_falls_back_to_memory() {
    let (fake, vault) = vault(Faults {
        max_len: Some(1280),
        deny_set: Some(("#".into(), "denied (-25308)".into())),
        ..Faults::default()
    });
    let k = key("c", "local", "pem");
    let long = "x".repeat(5000);
    let set = vault.set(&k, &long).unwrap();
    assert_eq!(set.backend, SecretBackend::Memory);
    assert!(set.warning.is_some());
    assert_eq!(vault.get(&k).unwrap().unwrap().value, long);
    assert!(
        accounts(&fake).is_empty(),
        "no partial chunks left: {:?}",
        accounts(&fake)
    );
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
fn secret_status_lists_names_and_backends_only() {
    let dir = collection_with_secret_env();
    let (_fake, vault) = vault(Faults {
        deny_set: Some(("/host".into(), "denied (-128)".into())),
        ..Faults::default()
    });
    let ws = Workspace::with_secrets(ExecOptions::default(), Arc::new(vault));
    let id = ws.open(dir.path()).unwrap();
    let open = ws.get(&id).unwrap();
    let warnings = open
        .update_secrets(
            "local",
            [
                ("token".to_owned(), Some("t".to_owned())),
                ("host".to_owned(), Some("h".to_owned())),
            ]
            .into(),
        )
        .unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("host"));
    assert_eq!(
        open.secret_status("local"),
        [
            ("host".to_owned(), SecretBackend::Memory),
            ("token".to_owned(), SecretBackend::Keychain)
        ]
    );
    open.update_secrets("local", [("token".to_owned(), None)].into())
        .unwrap();
    assert_eq!(
        open.secret_status("local"),
        [("host".to_owned(), SecretBackend::Memory)]
    );
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
    let (_fake, vault) = vault(Faults::default());
    let vault = Arc::new(vault);
    let ws = Workspace::with_secrets(ExecOptions::default(), vault.clone());
    let id = ws.open(dir.path()).unwrap();
    // Stored earlier (e.g. by a previous run of the app): read from the keychain.
    vault
        .set(&key(&id.0, "local", "host"), &server.uri())
        .unwrap();
    let open = ws.get(&id).unwrap();
    assert_eq!(
        open.secret_status("local"),
        [("host".to_owned(), SecretBackend::Keychain)]
    );
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
    let (_fake, vault) = vault(Faults::default());
    let id = CollectionId::for_root(dir.path()).unwrap();
    vault.set(&key(&id.0, "local", "token"), "kc").unwrap();
    vault.set(&key(&id.0, "local", "undeclared"), "x").unwrap();
    let values = plover_core::secrets::env_secrets(&vault, &collection, "local");
    assert_eq!(values, [("token".to_owned(), "kc".to_owned())].into());
}
