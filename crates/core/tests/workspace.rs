use std::fs;
use std::path::{Path, PathBuf};

use brs_core::executor::ExecOptions;
use brs_core::runner::{SendInput, send_one};
use brs_core::store::{self, ContentHash, StoreError};
use brs_core::workspace::Workspace;
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
    let hash = store::save_request(&root, ECHO_JSON, &request, &stored.hash).unwrap();
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
    open.set_secrets("local", [("host".to_owned(), server.uri())].into());
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
    assert_eq!(res.status, 201);
    assert_eq!(res.body, b"ok");
}
