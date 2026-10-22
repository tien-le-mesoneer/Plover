use std::fs;
use std::path::{Path, PathBuf};

use brs_core::collection::{Collection, Item};

fn testbench() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bruno-tests/collection")
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn request(name: &str, seq: u32) -> String {
    format!(
        "meta {{\n  name: {name}\n  type: http\n  seq: {seq}\n}}\n\nget {{\n  url: http://h/{name}\n}}\n"
    )
}

fn folder(seq: u32) -> String {
    format!("meta {{\n  seq: {seq}\n}}\n")
}

fn names(c: &Collection, target: &Path) -> Vec<String> {
    c.requests_under(target)
        .iter()
        .map(|r| {
            r.path
                .strip_prefix(&c.root)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn loads_bruno_testbench() {
    let c = Collection::load(&testbench()).unwrap();
    assert_eq!(c.config.name, "bruno-testbench");
    assert!(
        c.config.extra.contains_key("proxy"),
        "unknown bruno.json fields kept"
    );
    assert!(!c.environments.is_empty());
    assert!(c.scope.is_some());
    let all = c.requests_under(&c.root);
    let invalid: Vec<_> = all
        .iter()
        .filter(|r| matches!(r.item, Item::Invalid { .. }))
        .map(|r| r.path)
        .collect();
    assert!(invalid.is_empty(), "invalid: {invalid:?}");
    assert!(all.len() > 100, "only {} requests", all.len());
}

#[test]
fn find_root_from_nested_file() {
    let root = Collection::find_root(&testbench().join("ping.bru")).unwrap();
    assert_eq!(root, std::path::absolute(testbench()).unwrap());
}

#[test]
fn ordering_folders_then_requests_by_seq() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"t","type":"collection"}"#,
    );
    write(root, "b/folder.bru", &folder(1));
    write(root, "b/x.bru", &request("x", 1));
    write(root, "a/folder.bru", &folder(2));
    write(root, "a/y.bru", &request("y", 1));
    write(root, "r2.bru", &request("r2", 2));
    write(root, "r1.bru", &request("r1", 1));
    write(root, "environments/local.bru", "vars {\n  host: h\n}\n");
    write(root, "node_modules/z.bru", &request("z", 1));
    let c = Collection::load(root).unwrap();
    assert_eq!(names(&c, root), ["b/x.bru", "a/y.bru", "r1.bru", "r2.bru"]);
    assert_eq!(names(&c, &root.join("a")), ["a/y.bru"]);
    assert_eq!(names(&c, &root.join("r2.bru")), ["r2.bru"]);
    assert_eq!(c.environment("local").unwrap().file.vars[0].value, "h");
    let refs = c.requests_under(&root.join("b/x.bru"));
    assert_eq!(refs[0].folders.len(), 1);
}

#[test]
fn invalid_file_is_item_not_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"t","type":"collection"}"#,
    );
    write(root, "broken.bru", "get {\n");
    // A known block with content Bruno's grammar rejects fails the file (collection.js:74-79).
    write(
        root,
        "bad-headers.bru",
        "get {\n  url: http://h\n}\n\nheaders {\n  x-a 1\n}\n",
    );
    write(root, "ok.bru", &request("ok", 1));
    let c = Collection::load(root).unwrap();
    let refs = c.requests_under(root);
    assert_eq!(refs.len(), 3);
    assert!(matches!(refs[1].item, Item::Invalid { .. }));
    assert!(matches!(refs[2].item, Item::Invalid { .. }));
}

#[test]
fn missing_config_is_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Collection::load(dir.path()).is_err());
    assert!(Collection::find_root(dir.path()).is_err());
}
