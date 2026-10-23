use std::fs;
use std::path::{Path, PathBuf};

use plover_core::collection::{Collection, Item};

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

// utils/collection.js:85-87: folders (sortByNameThenSequence) first, then requests by seq.
// Names are chosen so that alphabetical order differs from seq order.
#[test]
fn ordering_folders_then_requests_by_seq() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"t","type":"collection"}"#,
    );
    write(root, "z/folder.bru", &folder(1));
    write(root, "z/x.bru", &request("x", 1));
    write(root, "a/folder.bru", &folder(2));
    write(root, "a/y.bru", &request("y", 1));
    write(root, "z.bru", &request("z", 1));
    write(root, "a.bru", &request("a", 2));
    write(root, "environments/local.bru", "vars {\n  host: h\n}\n");
    write(root, "node_modules/n.bru", &request("n", 1));
    let c = Collection::load(root).unwrap();
    assert_eq!(names(&c, root), ["z/x.bru", "a/y.bru", "z.bru", "a.bru"]);
    assert_eq!(names(&c, &root.join("a")), ["a/y.bru"]);
    assert_eq!(names(&c, &root.join("a.bru")), ["a.bru"]);
    assert_eq!(c.environment("local").unwrap().file.vars[0].value, "h");
    let refs = c.requests_under(&root.join("z/x.bru"));
    assert_eq!(refs[0].folders.len(), 1);
}

// bruno-common/src/collection/index.ts:10 sorts folder names with `localeCompare`
// (ICU root collation): punctuation < digits < letters, case-insensitive first.
#[test]
fn folders_without_seq_use_locale_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"t","type":"collection"}"#,
    );
    // (`beta` vs `Beta` can't both exist on case-insensitive filesystems; the
    // lowercase-first tie-break is covered by the unit test in collection.rs.)
    for name in ["Beta", "alpha", "01-users", "_setup", "-dash"] {
        write(root, &format!("{name}/r.bru"), &request(name, 1));
    }
    let c = Collection::load(root).unwrap();
    assert_eq!(
        names(&c, root),
        [
            "_setup/r.bru",
            "-dash/r.bru",
            "01-users/r.bru",
            "alpha/r.bru",
            "Beta/r.bru"
        ]
    );
}

// utils/collection.js:61-63 uses lstat: a symlinked directory is not a folder, while a
// symlinked .bru file is read through the link.
#[cfg(unix)]
#[test]
fn symlinked_dirs_are_not_folders() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"t","type":"collection"}"#,
    );
    write(root, "a.bru", &request("a", 1));
    write(root, "real/b.bru", &request("b", 1));
    std::os::unix::fs::symlink(".", root.join("loop")).unwrap();
    std::os::unix::fs::symlink(root.join("real"), root.join("linked")).unwrap();
    std::os::unix::fs::symlink(root.join("a.bru"), root.join("s.bru")).unwrap();
    let c = Collection::load(root).unwrap();
    assert_eq!(names(&c, root), ["real/b.bru", "a.bru", "s.bru"]);
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
