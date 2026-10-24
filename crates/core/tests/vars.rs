//! Variable sources for the hover card: which scope wins and which it shadows.

use std::fs;
use std::path::Path;

use plover_core::executor::ExecOptions;
use plover_core::request::{HttpRequest, KeyValue};
use plover_core::runner::{SendInput, preview_stack};
use plover_core::vars::{ValueMap, VarMap, VarSource, VarStack};
use plover_core::workspace::Workspace;
use serde_json::json;

fn map(pairs: &[(&str, &str)]) -> VarMap {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

#[test]
fn entries_report_winner_and_shadows() {
    let stack = VarStack::new(
        map(&[("host", "runtime")]),
        VarMap::new(),
        vec![],
        map(&[("host", "env"), ("only", "e")]),
        map(&[("host", "coll")]),
    );
    let entries = stack.entries();
    let host = entries.iter().find(|e| e.name == "host").unwrap();
    assert_eq!(host.source, VarSource::Runtime);
    assert_eq!(host.value, json!("runtime"));
    assert_eq!(
        host.shadows,
        vec![VarSource::Environment, VarSource::Collection]
    );
    let only = entries.iter().find(|e| e.name == "only").unwrap();
    assert_eq!(only.source, VarSource::Environment);
    assert!(only.shadows.is_empty());
}

#[test]
fn process_env_on_top() {
    let mut env = ValueMap::new();
    env.insert("PLOVER_X".into(), json!("from-env"));
    let stack = VarStack::new(
        VarMap::new(),
        VarMap::new(),
        vec![],
        VarMap::new(),
        VarMap::new(),
    )
    .with_process_env(&env);
    let entries = stack.entries();
    let e = entries
        .iter()
        .find(|e| e.name == "process.env.PLOVER_X")
        .unwrap();
    assert_eq!(e.source, VarSource::ProcessEnv);
    assert_eq!(
        stack.lookup("process.env.PLOVER_X").as_deref(),
        Some("from-env")
    );
}

#[test]
fn folder_layers_are_labelled() {
    let stack = VarStack::new(
        VarMap::new(),
        map(&[("a", "req")]),
        vec![map(&[("a", "outer"), ("f", "1")]), map(&[("a", "inner")])],
        VarMap::new(),
        VarMap::new(),
    );
    let entries = stack.entries();
    let a = entries.iter().find(|e| e.name == "a").unwrap();
    assert_eq!(a.source, VarSource::Request);
    assert_eq!(a.shadows, vec![VarSource::Folder]);
    assert_eq!(
        entries.iter().find(|e| e.name == "f").unwrap().source,
        VarSource::Folder
    );
}

fn collection() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"v","type":"collection"}"#,
    );
    write(
        root,
        "collection.bru",
        "vars:pre-request {\n  base: https://example.test\n}\n",
    );
    write(
        root,
        "auth/folder.bru",
        "meta {\n  name: auth\n}\n\nvars:pre-request {\n  realm: users\n}\n",
    );
    write(
        root,
        "auth/login.bru",
        "meta {\n  name: login\n  type: http\n  seq: 1\n}\n\npost {\n  url: {{base}}/login\n}\n\nvars:pre-request {\n  x: file\n}\n",
    );
    dir
}

#[test]
fn preview_uses_draft_vars_pre() {
    let dir = collection();
    let ws = Workspace::new(ExecOptions::default());
    let id = ws.open(dir.path()).unwrap();
    let open = ws.get(&id).unwrap();
    let mut draft: HttpRequest = open
        .collection()
        .requests_under(&dir.path().join("auth/login.bru"))
        .first()
        .map(|r| match r.item {
            plover_core::collection::Item::Request { file, .. } => file.request.clone(),
            _ => unreachable!(),
        })
        .unwrap();
    draft.vars_pre = vec![KeyValue::new("x", "draft")];
    let stack = preview_stack(
        &open,
        &SendInput {
            item_id: Some("auth/login.bru"),
            draft: Some(&draft),
            env: None,
        },
    )
    .unwrap();
    let entries = stack.entries();
    let x = entries.iter().find(|e| e.name == "x").unwrap();
    assert_eq!(x.source, VarSource::Request);
    assert_eq!(x.value, json!("draft"));
    assert_eq!(
        entries.iter().find(|e| e.name == "realm").unwrap().source,
        VarSource::Folder
    );
    assert_eq!(
        entries.iter().find(|e| e.name == "base").unwrap().source,
        VarSource::Collection
    );
}

#[test]
fn preview_scratch_has_no_folder_layer() {
    let dir = collection();
    let ws = Workspace::new(ExecOptions::default());
    let id = ws.open(dir.path()).unwrap();
    let open = ws.get(&id).unwrap();
    let draft = HttpRequest::blank();
    let stack = preview_stack(
        &open,
        &SendInput {
            item_id: None,
            draft: Some(&draft),
            env: None,
        },
    )
    .unwrap();
    let entries = stack.entries();
    assert!(entries.iter().all(|e| e.source != VarSource::Folder));
    assert!(entries.iter().any(|e| e.name == "base"));
}
