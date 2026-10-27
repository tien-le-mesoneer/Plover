//! Remembered collections and UI layout (`state.json`), and the workspace restoring them.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use plover_core::app_state::{AppStateStore, MAX_RECENT, UiState};
use plover_core::executor::ExecOptions;
use plover_core::workspace::{CollectionId, Workspace};

fn collection(dir: &Path, name: &str) -> std::path::PathBuf {
    let root = dir.join(name);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("bruno.json"),
        format!(r#"{{"version":"1","name":"{name}","type":"collection"}}"#),
    )
    .unwrap();
    fs::write(
        root.join("ping.bru"),
        "meta {\n  name: ping\n  seq: 1\n}\n\nget {\n  url: http://127.0.0.1:9/\n}\n",
    )
    .unwrap();
    root
}

fn ui(tabs: &[&str]) -> UiState {
    UiState {
        tabs: tabs.iter().map(|s| s.to_string()).collect(),
        active: tabs.first().map(|s| s.to_string()),
        expanded: vec!["users".into()],
        env: Some("local".into()),
        ..UiState::default()
    }
}

#[test]
fn round_trip_keeps_open_order_recent_and_ui() {
    let dir = tempfile::tempdir().unwrap();
    let store = AppStateStore::load(dir.path());
    store.record_open("/a", "A");
    store.record_open("/b", "B");
    store.set_ui("/a", ui(&["ping.bru"])).unwrap();

    let again = AppStateStore::load(dir.path()).snapshot();
    assert_eq!(again.version, 1);
    let open: Vec<_> = again.open.iter().map(|o| o.root.as_str()).collect();
    assert_eq!(open, ["/a", "/b"]);
    let recent: Vec<_> = again.recent.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(recent, ["B", "A"], "newest first");
    assert_eq!(again.ui.get("/a"), Some(&ui(&["ping.bru"])));
}

#[test]
fn missing_file_is_empty_and_in_memory_never_writes() {
    let dir = tempfile::tempdir().unwrap();
    let store = AppStateStore::load(dir.path());
    assert!(store.snapshot().open.is_empty());
    assert!(
        !dir.path().join("state.json").exists(),
        "loading alone writes nothing"
    );

    let memory = AppStateStore::in_memory();
    memory.record_open("/a", "A");
    assert_eq!(memory.snapshot().open.len(), 1);
}

#[test]
fn corrupt_or_unknown_version_is_backed_up_and_replaced() {
    for bad in ["{not json", r#"{"version": 99, "open": []}"#] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("state.json"), bad).unwrap();
        let store = AppStateStore::load(dir.path());
        assert!(store.snapshot().open.is_empty(), "{bad}");
        let backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("state.json.bak-"))
            .collect();
        assert_eq!(backups.len(), 1, "{bad}: {backups:?}");
        let kept = fs::read_to_string(dir.path().join(&backups[0])).unwrap();
        assert_eq!(kept, bad, "the original bytes are kept");
        store.record_open("/a", "A");
        assert!(AppStateStore::load(dir.path()).snapshot().open.len() == 1);
    }
}

#[test]
fn recent_is_capped_deduped_and_close_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = AppStateStore::load(dir.path());
    for i in 0..MAX_RECENT + 3 {
        store.record_open(&format!("/c{i}"), &format!("C{i}"));
    }
    store.record_open("/c5", "C5");
    let s = store.snapshot();
    assert_eq!(s.recent.len(), MAX_RECENT);
    assert_eq!(s.recent[0].root, "/c5", "reopening moves it to the front");
    assert_eq!(s.recent.iter().filter(|r| r.root == "/c5").count(), 1);
    assert_eq!(s.open.iter().filter(|o| o.root == "/c5").count(), 1);

    store.record_close("/c5");
    let s = store.snapshot();
    assert!(s.open.iter().all(|o| o.root != "/c5"));
    assert_eq!(s.recent[0].root, "/c5");
    store.remove_recent("/c5");
    assert!(store.snapshot().recent.iter().all(|r| r.root != "/c5"));
}

#[test]
fn ui_state_limits_are_enforced() {
    let store = AppStateStore::in_memory();
    let too_many = UiState {
        tabs: (0..51).map(|i| format!("r{i}.bru")).collect(),
        ..UiState::default()
    };
    assert!(store.set_ui("/a", too_many).is_err());
    let too_long = UiState {
        expanded: vec!["x".repeat(1025)],
        ..UiState::default()
    };
    assert!(store.set_ui("/a", too_long).is_err());
    assert!(store.ui("/a").is_none());
}

#[test]
fn workspace_restores_in_order_and_reports_missing_roots() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join("state");
    let a = collection(dir.path(), "alpha");
    let b = collection(dir.path(), "beta");
    {
        let ws = Workspace::new(ExecOptions::default())
            .with_app_state(Arc::new(AppStateStore::load(&state_dir)));
        ws.open(&b).unwrap();
        ws.open(&a).unwrap();
    }
    fs::remove_dir_all(&b).unwrap();

    let ws = Workspace::new(ExecOptions::default())
        .with_app_state(Arc::new(AppStateStore::load(&state_dir)));
    ws.restore();
    let names: Vec<_> = ws
        .list()
        .iter()
        .map(|o| o.collection().config.name.clone())
        .collect();
    assert_eq!(names, ["alpha"]);
    let missing = ws.unavailable();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].name, "beta");
    assert!(!missing[0].reason.is_empty());
    // The ID stays the one the collection had while it existed.
    let a_id = CollectionId::for_root(&a).unwrap();
    assert_eq!(ws.list()[0].id, a_id);

    // Retry after the folder comes back: it opens and is no longer unavailable.
    collection(dir.path(), "beta");
    let id = ws.open(&b).unwrap();
    assert!(ws.unavailable().is_empty());
    assert_eq!(id, missing[0].id);

    // Closing forgets it for the next start but keeps it in recent.
    assert!(ws.close(&id));
    let s = AppStateStore::load(&state_dir).snapshot();
    assert_eq!(s.open.len(), 1);
    assert!(s.recent.iter().any(|r| r.name == "beta"));
}

#[test]
fn removing_an_unavailable_entry_forgets_it() {
    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join("state");
    let gone = collection(dir.path(), "gone");
    Workspace::new(ExecOptions::default())
        .with_app_state(Arc::new(AppStateStore::load(&state_dir)))
        .open(&gone)
        .unwrap();
    fs::remove_dir_all(&gone).unwrap();
    let ws = Workspace::new(ExecOptions::default())
        .with_app_state(Arc::new(AppStateStore::load(&state_dir)));
    ws.restore();
    let id = ws.unavailable()[0].id.clone();
    assert!(ws.close(&id));
    assert!(ws.unavailable().is_empty());
    assert!(AppStateStore::load(&state_dir).snapshot().open.is_empty());
}

#[test]
fn layout_defaults_and_round_trips() {
    use plover_core::app_state::LayoutState;
    let dir = tempfile::tempdir().unwrap();
    // A state file written before `layout` existed still loads.
    fs::write(
        dir.path().join("state.json"),
        r#"{"version":1,"open":[],"recent":[],"ui":{}}"#,
    )
    .unwrap();
    let store = AppStateStore::load(dir.path());
    assert_eq!(store.layout(), LayoutState::default());
    store
        .set_layout(LayoutState {
            sidebar_width: Some(320),
            ..LayoutState::default()
        })
        .unwrap();
    let again = AppStateStore::load(dir.path());
    assert_eq!(again.layout().sidebar_width, Some(320));
    assert!(
        again
            .set_layout(LayoutState {
                sidebar_width: Some(50),
                ..LayoutState::default()
            })
            .is_err()
    );
    assert_eq!(again.layout().sidebar_width, Some(320));
}

#[test]
fn ui_state_without_collapsed_reads_as_expanded() {
    let ui: UiState =
        serde_json::from_str(r#"{"tabs":[],"active":null,"expanded":[],"env":null}"#).unwrap();
    assert!(!ui.collapsed);
    let folded: UiState = serde_json::from_str(r#"{"collapsed":true}"#).unwrap();
    assert!(folded.collapsed);
}

#[test]
fn layout_rejects_split_outside_200_800() {
    use plover_core::app_state::LayoutState;
    for bad in [199, 801] {
        let side = LayoutState {
            split_permille: Some(bad),
            ..LayoutState::default()
        };
        assert!(side.validate().is_err(), "{bad}");
        let stacked = LayoutState {
            stacked_split_permille: Some(bad),
            ..LayoutState::default()
        };
        assert!(stacked.validate().is_err(), "{bad}");
    }
    for good in [Some(200), Some(800), None] {
        let l = LayoutState {
            split_permille: good,
            stacked_split_permille: good,
            ..LayoutState::default()
        };
        assert!(l.validate().is_ok(), "{good:?}");
    }
}

#[test]
fn layout_without_split_fields_reads_as_side_500() {
    use plover_core::app_state::{LayoutState, SplitOrientation};
    let old: LayoutState = serde_json::from_str(r#"{"sidebarWidth":300}"#).unwrap();
    assert_eq!(old.split_orientation, None);
    assert_eq!(old.split_permille, None);
    assert_eq!(old.stacked_split_permille, None);
    let new: LayoutState =
        serde_json::from_str(r#"{"splitOrientation":"stacked","stackedSplitPermille":300}"#)
            .unwrap();
    assert_eq!(new.split_orientation, Some(SplitOrientation::Stacked));
    assert_eq!(
        serde_json::to_value(SplitOrientation::Side).unwrap(),
        serde_json::json!("side")
    );
}
