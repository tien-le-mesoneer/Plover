use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use plover_core::run_history::{HistoryError, KEEP_RUNS, RunHistory};
use plover_core::run_record::{
    RUN_RECORD_VERSION, RunHeader, RunId, RunOutcome, RunRecord, RunSource,
};
use plover_core::runner::RunReport;
use plover_core::workspace::CollectionId;

fn record(root: &str, at_secs: u64) -> RunRecord {
    let at = UNIX_EPOCH + Duration::from_secs(at_secs);
    RunRecord {
        version: RUN_RECORD_VERSION,
        header: RunHeader {
            id: RunId::new(at),
            source: RunSource::Plv,
            collection_root: root.into(),
            collection_name: "c".into(),
            target: ".".into(),
            env: None,
            started_at_ms: at_secs * 1000,
        },
        outcome: RunOutcome::Completed,
        report: RunReport {
            passed: 1,
            ..RunReport::default()
        },
        results: Vec::new(),
    }
}

fn key(root: &str) -> CollectionId {
    CollectionId::of_canonical(root)
}

fn ids(history: &RunHistory, root: &str) -> Vec<RunId> {
    history
        .list(&key(root))
        .into_iter()
        .map(|m| m.header.id)
        .collect()
}

fn save_list_load_with(history: &RunHistory) {
    let a = record("/c", 1_000);
    let b = record("/c", 2_000);
    history.save(&a).unwrap();
    history.save(&b).unwrap();
    assert_eq!(
        ids(history, "/c"),
        [b.header.id.clone(), a.header.id.clone()]
    );
    assert_eq!(history.load(&key("/c"), &a.header.id).unwrap(), a);
    assert_eq!(history.list(&key("/c"))[0].report.passed, 1);
}

fn keeps_twenty_with(history: &RunHistory) {
    let records: Vec<_> = (0..23).map(|i| record("/k", 1_000 + i)).collect();
    for r in &records {
        history.save(r).unwrap();
    }
    let listed = ids(history, "/k");
    assert_eq!(listed.len(), KEEP_RUNS);
    for old in &records[..3] {
        assert!(!listed.contains(&old.header.id));
        assert!(matches!(
            history.load(&key("/k"), &old.header.id),
            Err(HistoryError::NotFound(_))
        ));
    }
}

#[test]
fn save_list_load() {
    let dir = tempfile::tempdir().unwrap();
    let history = RunHistory::at(dir.path());
    save_list_load_with(&history);
    let folder = dir.path().join("runs").join(key("/c").0);
    let names: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(
        names.iter().filter(|n| n.ends_with(".meta.json")).count(),
        2
    );
    assert_eq!(names.len(), 4, "{names:?}");
}

#[test]
fn keeps_twenty() {
    let dir = tempfile::tempdir().unwrap();
    keeps_twenty_with(&RunHistory::at(dir.path()));
}

#[test]
fn in_memory_behaves_the_same() {
    save_list_load_with(&RunHistory::in_memory());
    keeps_twenty_with(&RunHistory::in_memory());
}

#[test]
fn corrupt_and_future_files_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let history = RunHistory::at(dir.path());
    let folder = dir.path().join("runs").join(key("/c").0);
    std::fs::create_dir_all(&folder).unwrap();
    let garbage = RunId::new(UNIX_EPOCH + Duration::from_secs(10));
    let future = RunId::new(UNIX_EPOCH + Duration::from_secs(11));
    for ext in [".json", ".meta.json"] {
        std::fs::write(folder.join(format!("{garbage}{ext}")), "{not json").unwrap();
    }
    let mut v = serde_json::to_value(record("/c", 11)).unwrap();
    v["version"] = 99.into();
    v["id"] = future.as_str().into();
    for ext in [".json", ".meta.json"] {
        std::fs::write(folder.join(format!("{future}{ext}")), v.to_string()).unwrap();
    }
    std::fs::write(folder.join("notes.txt"), "ignored").unwrap();
    assert!(history.list(&key("/c")).is_empty());
    for id in [&garbage, &future] {
        assert!(matches!(
            history.load(&key("/c"), id),
            Err(HistoryError::Unreadable { .. })
        ));
    }
    for i in 0..KEEP_RUNS as u64 {
        history.save(&record("/c", 1_000 + i)).unwrap();
    }
    assert_eq!(history.list(&key("/c")).len(), KEEP_RUNS);
    for id in [&garbage, &future] {
        assert!(!folder.join(format!("{id}.json")).exists());
        assert!(!folder.join(format!("{id}.meta.json")).exists());
    }
    assert!(folder.join("notes.txt").exists());
}

#[test]
fn concurrent_saves_and_prunes() {
    let dir = tempfile::tempdir().unwrap();
    let histories = [
        Arc::new(RunHistory::at(dir.path())),
        Arc::new(RunHistory::at(dir.path())),
    ];
    let threads: Vec<_> = (0..16)
        .map(|t| {
            let history = histories[t % 2].clone();
            std::thread::spawn(move || {
                for _ in 0..5 {
                    let mut r = record("/p", 0);
                    r.header.id = RunId::new(SystemTime::now());
                    history.save(&r).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let history = &histories[0];
    let listed = ids(history, "/p");
    assert_eq!(listed.len(), KEEP_RUNS);
    for id in &listed {
        history.load(&key("/p"), id).unwrap();
    }
}

#[test]
fn meta_written_last() {
    let dir = tempfile::tempdir().unwrap();
    let history = RunHistory::at(dir.path());
    let folder = dir.path().join("runs").join(key("/c").0);
    std::fs::create_dir_all(&folder).unwrap();
    let r = record("/c", 5);
    std::fs::write(
        folder.join(format!("{}.json", r.header.id)),
        serde_json::to_vec(&r).unwrap(),
    )
    .unwrap();
    assert!(history.list(&key("/c")).is_empty());
}

#[test]
fn find_across_collections() {
    let dir = tempfile::tempdir().unwrap();
    let history = RunHistory::at(dir.path());
    let a = record("/a", 1);
    let b = record("/b", 2);
    history.save(&a).unwrap();
    history.save(&b).unwrap();
    assert_eq!(history.find(&b.header.id).unwrap(), b);
    let unknown = RunId::new(UNIX_EPOCH + Duration::from_secs(99));
    assert!(matches!(
        history.find(&unknown),
        Err(HistoryError::NotFound(_))
    ));
    let empty = tempfile::tempdir().unwrap();
    assert!(matches!(
        RunHistory::at(empty.path()).find(&unknown),
        Err(HistoryError::NotFound(_))
    ));
}

#[test]
fn clear_deletes_only_that_collection() {
    let dir = tempfile::tempdir().unwrap();
    let history = RunHistory::at(dir.path());
    history.save(&record("/a", 1)).unwrap();
    history.save(&record("/a", 2)).unwrap();
    history.save(&record("/b", 3)).unwrap();
    assert_eq!(history.clear(&key("/a")).unwrap(), 2);
    assert!(history.list(&key("/a")).is_empty());
    assert_eq!(history.list(&key("/b")).len(), 1);
    assert_eq!(history.clear(&key("/none")).unwrap(), 0);
}
