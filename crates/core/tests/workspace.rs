use std::fs;
use std::path::{Path, PathBuf};

use plover_core::executor::ExecOptions;
use plover_core::runner::{SendInput, send_one};
use plover_core::store::{self, ContentHash, StoreError};
use plover_core::workspace::Workspace;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/bruno-tests/collection"
);
const ECHO_JSON: &str = "echo/echo json.bru";

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_dir(&entry.path(), &target);
        } else if kind.is_file() {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn fixture_copy() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("collection");
    copy_dir(Path::new(FIXTURE), &root);
    (dir, root)
}

fn changed_lines(before: &str, after: &str) -> (usize, usize) {
    let diff = similar::TextDiff::from_lines(before, after);
    let mut removed = 0;
    let mut added = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Delete => removed += 1,
            similar::ChangeTag::Insert => added += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    (removed, added)
}

#[test]
fn open_is_idempotent() {
    let (_dir, root) = fixture_copy();
    let ws = Workspace::new(ExecOptions::default());
    let a = ws.open(&root).unwrap();
    let b = ws.open(&root.join("ping.bru")).unwrap();
    assert_eq!(a, b);
    assert_eq!(ws.list().len(), 1);
}

#[test]
fn item_path_rejects_escape() {
    let (dir, root) = fixture_copy();
    let outside = dir.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("x.bru"), "get {\n  url: x\n}\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
    for id in [
        "../x.bru",
        "/etc/passwd",
        "a/../../x.bru",
        "",
        "a//b.bru",
        "./ping.bru",
        "link/x.bru",
    ] {
        let err = store::resolve_item_path(&root, id).unwrap_err();
        assert!(
            matches!(err, StoreError::OutsideRoot(_)),
            "{id:?} gave {err:?}"
        );
    }
    let ok = store::resolve_item_path(&root, ECHO_JSON).unwrap();
    assert_eq!(ok, root.join("echo").join("echo json.bru"));
}

#[test]
fn save_minimal_diff() {
    let (_dir, root) = fixture_copy();
    let before = fs::read_to_string(root.join(ECHO_JSON)).unwrap();
    let stored = store::read_request(&root, ECHO_JSON).unwrap();
    assert_eq!(stored.hash, ContentHash::of(before.as_bytes()));
    let mut request = stored.value;
    request.headers[0].value = "baz".into();
    let hash = store::save_request(&root, ECHO_JSON, &request, &stored.hash)
        .unwrap()
        .hash;
    let after = fs::read_to_string(root.join(ECHO_JSON)).unwrap();
    assert_eq!(changed_lines(&before, &after), (1, 1));
    assert!(after.contains("  foo: baz\n"));
    assert_eq!(hash, ContentHash::of(after.as_bytes()));
}

#[test]
fn save_conflict_does_not_write() {
    let (_dir, root) = fixture_copy();
    let stored = store::read_request(&root, ECHO_JSON).unwrap();
    let external = fs::read_to_string(root.join(ECHO_JSON))
        .unwrap()
        .replace("foo: bar", "foo: external");
    fs::write(root.join(ECHO_JSON), &external).unwrap();
    let mut request = stored.value;
    request.headers[0].value = "mine".into();
    let err = store::save_request(&root, ECHO_JSON, &request, &stored.hash).unwrap_err();
    let StoreError::Conflict { current } = err else {
        panic!("expected conflict, got {err:?}");
    };
    assert_eq!(current.value, external);
    assert_eq!(current.hash, ContentHash::of(external.as_bytes()));
    assert_eq!(fs::read_to_string(root.join(ECHO_JSON)).unwrap(), external);
}

#[test]
fn save_environment_with_hash() {
    let (_dir, root) = fixture_copy();
    let ws = Workspace::new(ExecOptions::default());
    let id = ws.open(&root).unwrap();
    let open = ws.get(&id).unwrap();
    let env = open.collection().environment("Local").unwrap().clone();
    let before = fs::read_to_string(&env.path).unwrap();
    let mut vars = env.file.vars.clone();
    vars[0].value = "http://127.0.0.1:9".into();
    let hash = store::save_environment(&root, "Local", &vars, &ContentHash::of(before.as_bytes()))
        .unwrap();
    let after = fs::read_to_string(&env.path).unwrap();
    assert_eq!(changed_lines(&before, &after), (1, 1));
    assert_eq!(hash, ContentHash::of(after.as_bytes()));
    let err =
        store::save_environment(&root, "Local", &vars, &ContentHash::of(b"stale")).unwrap_err();
    assert!(matches!(err, StoreError::Conflict { .. }));
    let err = store::save_environment(&root, "../Local", &vars, &hash).unwrap_err();
    assert!(matches!(err, StoreError::OutsideRoot(_)));
}

#[tokio::test]
async fn send_one_uses_draft_and_secrets() {
    let server = MockServer::start().await;
    Mock::given(path("/draft"))
        .respond_with(ResponseTemplate::new(201).set_body_string("ok"))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("bruno.json"),
        r#"{"version":"1","name":"t","type":"collection"}"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("environments")).unwrap();
    fs::write(
        root.join("environments/local.bru"),
        "vars {\n  other: 1\n}\nvars:secret [\n  host\n]\n",
    )
    .unwrap();
    fs::write(
        root.join("r.bru"),
        "meta {\n  name: r\n  type: http\n  seq: 1\n}\n\nget {\n  url: {{host}}/disk\n}\n",
    )
    .unwrap();
    let ws = Workspace::new(ExecOptions::default());
    let id = ws.open(root).unwrap();
    let open = ws.get(&id).unwrap();
    open.update_secrets("local", [("host".to_owned(), Some(server.uri()))].into())
        .unwrap();
    let mut draft = store::read_request(&open.root(), "r.bru").unwrap().value;
    draft.url = "{{host}}/draft".into();
    let res = send_one(
        &open,
        SendInput {
            item_id: "r.bru",
            draft: Some(&draft),
            env: Some("local"),
        },
    )
    .await
    .unwrap();
    let response = res.response.unwrap();
    assert_eq!(response.status, 201);
    assert_eq!(response.body, b"ok");
}

#[test]
fn concurrent_saves_with_same_base_hash_one_wins() {
    for round in 0..10 {
        let (_dir, root) = fixture_copy();
        let stored = store::read_request(&root, ECHO_JSON).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|k| {
                let root = root.clone();
                let hash = stored.hash.clone();
                let mut request = stored.value.clone();
                request.url = format!("http://race/{round}/{k}");
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store::save_request(&root, ECHO_JSON, &request, &hash)
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let ok = results.iter().filter(|r| r.is_ok()).count();
        let conflicts = results
            .iter()
            .filter(|r| matches!(r, Err(StoreError::Conflict { .. })))
            .count();
        assert_eq!((ok, conflicts), (1, 7), "round {round}");
    }
}

#[test]
fn save_returns_request_as_reread_from_disk() {
    let (_dir, root) = fixture_copy();
    let stored = store::read_request(&root, ECHO_JSON).unwrap();
    let mut request = stored.value;
    request.headers[0].name = "~x".into();
    request.headers[0].value = "  v  ".into();
    let saved = store::save_request(&root, ECHO_JSON, &request, &stored.hash).unwrap();
    // `~x` can only be written as a disabled `x`; values are trimmed on read.
    assert_eq!(saved.value.headers[0].name, "x");
    assert!(!saved.value.headers[0].enabled);
    assert_eq!(saved.value.headers[0].value, "v");
    assert_eq!(
        saved.value,
        store::read_request(&root, ECHO_JSON).unwrap().value
    );
    assert_eq!(
        saved.hash,
        ContentHash::of(&fs::read(root.join(ECHO_JSON)).unwrap())
    );
}

#[test]
fn inactive_bodies_survive_mode_switches() {
    use plover_core::request::Body;
    let (_dir, root) = fixture_copy();
    let before = fs::read_to_string(root.join(ECHO_JSON)).unwrap();
    let stored = store::read_request(&root, ECHO_JSON).unwrap();
    let json = stored.value.body.clone();
    assert!(matches!(json, Body::Json(_)));
    assert!(stored.value.other_bodies.is_empty());

    // json -> none: the body:json block stays, only the mode line changes.
    let mut none = stored.value.clone();
    none.other_bodies = vec![json.clone()];
    none.body = Body::None;
    let saved = store::save_request(&root, ECHO_JSON, &none, &stored.hash).unwrap();
    let after = fs::read_to_string(root.join(ECHO_JSON)).unwrap();
    assert_eq!(changed_lines(&before, &after), (1, 1), "{after}");
    assert!(after.contains("  body: none\n") && after.contains("body:json {"));
    assert_eq!(saved.value.other_bodies, vec![json.clone()]);

    // none -> json again: the file is back to the original bytes.
    let mut back = saved.value.clone();
    back.body = json;
    back.other_bodies.clear();
    store::save_request(&root, ECHO_JSON, &back, &saved.hash).unwrap();
    assert_eq!(fs::read_to_string(root.join(ECHO_JSON)).unwrap(), before);
}

#[test]
fn secrets_merge_and_clear() {
    let (_dir, root) = fixture_copy();
    let ws = Workspace::new(ExecOptions::default());
    let open = ws.get(&ws.open(&root).unwrap()).unwrap();
    open.update_secrets("Local", [("a".to_owned(), Some("1".to_owned()))].into())
        .unwrap();
    open.update_secrets("Local", [("b".to_owned(), Some("2".to_owned()))].into())
        .unwrap();
    assert_eq!(
        open.secrets("Local"),
        [
            ("a".to_owned(), "1".to_owned()),
            ("b".to_owned(), "2".to_owned())
        ]
        .into()
    );
    assert_eq!(names(&open.secret_status("Local")), ["a", "b"]);
    open.update_secrets("Local", [("a".to_owned(), None)].into())
        .unwrap();
    assert_eq!(names(&open.secret_status("Local")), ["b"]);
    assert!(open.secret_status("Prod").is_empty());
}

fn names(status: &[(String, plover_core::secrets::SecretBackend)]) -> Vec<&str> {
    status.iter().map(|(name, _)| name.as_str()).collect()
}

/// Collections are listed in the order they were opened (the UI sidebar order),
/// not by their hash IDs, so reloading the page keeps the same order.
#[test]
fn list_keeps_open_order() {
    let dir = tempfile::tempdir().unwrap();
    let roots: Vec<PathBuf> = ["alpha", "beta", "gamma"]
        .iter()
        .map(|name| {
            let root = dir.path().join(name);
            fs::create_dir_all(&root).unwrap();
            fs::write(
                root.join("bruno.json"),
                format!(r#"{{"version":"1","name":"{name}","type":"collection"}}"#),
            )
            .unwrap();
            root
        })
        .collect();
    for order in [[0, 1, 2], [2, 0, 1], [1, 2, 0]] {
        let ws = Workspace::new(ExecOptions::default());
        for i in order {
            ws.open(&roots[i]).unwrap();
        }
        // Re-opening an already open collection must not move it.
        ws.open(&roots[order[0]]).unwrap();
        let listed: Vec<String> = ws
            .list()
            .iter()
            .map(|c| c.collection().config.name.clone())
            .collect();
        let expected: Vec<String> = order
            .iter()
            .map(|&i| ["alpha", "beta", "gamma"][i].to_owned())
            .collect();
        assert_eq!(listed, expected);
    }
}
