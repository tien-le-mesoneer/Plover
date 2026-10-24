//! Names scripts set at runtime, found without running anything.

use std::fs;
use std::path::Path;

use plover_core::collection::Collection;
use plover_core::var_scan::scan_set_later;

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn req(name: &str, seq: u32, blocks: &str) -> String {
    format!(
        "meta {{\n  name: {name}\n  type: http\n  seq: {seq}\n}}\n\nget {{\n  url: https://example.test/{name}\n}}\n{blocks}"
    )
}

fn scan(files: &[(&str, &str)]) -> Vec<(String, String)> {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "bruno.json",
        r#"{"version":"1","name":"s","type":"collection"}"#,
    );
    for (rel, text) in files {
        write(dir.path(), rel, text);
    }
    let c = Collection::load(dir.path()).unwrap();
    scan_set_later(&c)
        .into_iter()
        .map(|s| (s.name, s.by))
        .collect()
}

#[test]
fn finds_set_var_set_env_var_and_post_response_vars() {
    let found = scan(&[(
        "a.bru",
        &req(
            "a",
            1,
            "\nscript:post-response {\n  bru.setVar(\"token\", res.body.t);\n  bru.setEnvVar('envOne', 1);\n  bru.setGlobalEnvVar(\"g\", 2);\n}\n\nvars:post-response {\n  docId: res.body.id\n}\n",
        ),
    )]);
    let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
    for n in ["token", "envOne", "g", "docId"] {
        assert!(names.contains(&n), "{n} in {names:?}");
    }
    assert!(found.iter().all(|(_, by)| by == "a.bru"));
}

#[test]
fn ignores_computed_names() {
    let found = scan(&[(
        "a.bru",
        &req(
            "a",
            1,
            "\nscript:pre-request {\n  bru.setVar(name, 1);\n  bru.setVar(`t${i}`, 1);\n}\n",
        ),
    )]);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn template_literal_without_interpolation_counts() {
    let found = scan(&[(
        "a.bru",
        &req(
            "a",
            1,
            "\nscript:pre-request {\n  bru.setVar(`token`, t);\n}\n",
        ),
    )]);
    assert_eq!(found, vec![("token".to_owned(), "a.bru".to_owned())]);
}

#[test]
fn first_setter_wins_in_run_order() {
    let found = scan(&[
        (
            "b.bru",
            &req(
                "b",
                2,
                "\nscript:post-response {\n  bru.setVar(\"token\", 2);\n}\n",
            ),
        ),
        (
            "a.bru",
            &req(
                "a",
                1,
                "\nscript:post-response {\n  bru.setVar(\"token\", 1);\n}\n",
            ),
        ),
        (
            "auth/folder.bru",
            "meta {\n  name: auth\n  seq: 3\n}\n\nscript:pre-request {\n  bru.setVar(\"realm\", 1);\n}\n",
        ),
        (
            "auth/login.bru",
            &req(
                "login",
                1,
                "\nscript:post-response {\n  bru.setVar(\"realm\", 2);\n}\n",
            ),
        ),
    ]);
    assert!(
        found.contains(&("token".to_owned(), "a.bru".to_owned())),
        "{found:?}"
    );
    assert!(
        found.contains(&("realm".to_owned(), "auth/folder.bru".to_owned())),
        "{found:?}"
    );
    assert_eq!(found.iter().filter(|(n, _)| n == "token").count(), 1);
}
