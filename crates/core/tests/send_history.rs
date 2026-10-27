//! The last 100 UI sends, masked before they are written.

use std::time::{Duration, UNIX_EPOCH};

use plover_core::run_record::{BodyDetail, RequestDetail, ResponseDetail, ResultStatus, RunId};
use plover_core::runner::RequestOutcome;
use plover_core::send_history::{KEEP_SENDS, SendHeader, SendHistory, SendRecord, send_result};
use plover_core::workspace::CollectionId;

fn text(t: &str) -> BodyDetail {
    BodyDetail::Text {
        text: t.to_owned(),
        size: t.len(),
        truncated: false,
    }
}

fn outcome(url: &str, headers: &[(&str, &str)], body: BodyDetail) -> RequestOutcome {
    RequestOutcome {
        status: Some(200),
        elapsed: Duration::from_millis(12),
        request: Some(RequestDetail {
            method: "POST".into(),
            url: url.into(),
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body,
        }),
        response: Some(ResponseDetail {
            status: 200,
            status_text: "OK".into(),
            headers: Vec::new(),
            body: text(r#"{"ok":true}"#),
            url: url.into(),
            size: 11,
            size_complete: true,
        }),
        ..RequestOutcome::default()
    }
}

fn header(root: &str, at_ms: u64) -> SendHeader {
    SendHeader {
        id: RunId::new(UNIX_EPOCH + Duration::from_millis(at_ms)),
        sent_at_ms: at_ms,
        collection_root: root.into(),
        collection_name: "c".into(),
        item: Some("users/me.bru".into()),
        env: Some("dev".into()),
    }
}

fn record(root: &str, at_ms: u64) -> SendRecord {
    let result = send_result(
        Some("users/me.bru"),
        "me",
        "POST",
        "http://h/me",
        &outcome("http://h/me", &[], BodyDetail::None),
        None,
    );
    SendRecord::masked(header(root, at_ms), result, &[])
}

fn ids(h: &SendHistory, c: Option<&CollectionId>) -> Vec<RunId> {
    h.list(c).into_iter().map(|m| m.header.id).collect()
}

#[test]
fn keeps_last_100_globally() {
    let dir = tempfile::tempdir().unwrap();
    let h = SendHistory::at(dir.path());
    let all: Vec<SendRecord> = (0..105)
        .map(|i| record(if i % 2 == 0 { "/a" } else { "/b" }, 1_000_000 + i * 1000))
        .collect();
    for r in &all {
        h.save(r).unwrap();
    }
    let listed = ids(&h, None);
    assert_eq!(listed.len(), KEEP_SENDS);
    for gone in &all[..5] {
        assert!(!listed.contains(&gone.header.id));
        assert!(h.load(&gone.header.id).is_err());
    }
    let files = std::fs::read_dir(dir.path().join("sends")).unwrap().count();
    assert_eq!(files, 2 * KEEP_SENDS);
}

#[test]
fn list_filters_by_collection_newest_first() {
    let h = SendHistory::in_memory();
    let a1 = record("/a", 1_000_000);
    let b1 = record("/b", 2_000_000);
    let a2 = record("/a", 3_000_000);
    for r in [&a1, &b1, &a2] {
        h.save(r).unwrap();
    }
    let a = CollectionId::of_canonical("/a");
    assert_eq!(
        ids(&h, Some(&a)),
        [a2.header.id.clone(), a1.header.id.clone()]
    );
    assert_eq!(
        ids(&h, None),
        [
            a2.header.id.clone(),
            b1.header.id.clone(),
            a1.header.id.clone()
        ]
    );
    let meta = &h.list(Some(&a))[0];
    assert_eq!((meta.method.as_str(), meta.name.as_str()), ("POST", "me"));
    assert_eq!(meta.status, ResultStatus::Pass);
    assert_eq!(meta.http_status, Some(200));
    assert_eq!(meta.duration_ms, 12);
    assert_eq!(h.load(&a2.header.id).unwrap(), a2);
}

#[test]
fn masked_removes_secret_from_headers_body_query() {
    let dir = tempfile::tempdir().unwrap();
    let h = SendHistory::at(dir.path());
    let secret = "sk-SEND-HISTORY-4242";
    let url = format!("http://h/me?key={secret}");
    let result = send_result(
        None,
        "scratch",
        "POST",
        &url,
        &outcome(
            &url,
            &[("Authorization", &format!("Bearer {secret}"))],
            text(&format!(r#"{{"apiKey":"{secret}"}}"#)),
        ),
        None,
    );
    let rec = SendRecord::masked(header("/a", 5_000_000), result, &[secret.to_owned()]);
    h.save(&rec).unwrap();
    for entry in std::fs::read_dir(dir.path().join("sends")).unwrap() {
        let bytes = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        assert!(!bytes.contains(secret), "{bytes}");
    }
    let loaded = h.load(&rec.header.id).unwrap();
    let request = loaded.result.outcome.request.unwrap();
    assert!(request.url.contains("***"));
    assert!(request.headers[0].1.contains("***"));
}

#[test]
fn bodies_over_1_mib_are_truncated() {
    let big = vec![b'x'; 2 << 20];
    let body = BodyDetail::of_bytes(&big, big.len(), false);
    let BodyDetail::Text {
        text,
        size,
        truncated,
    } = &body
    else {
        panic!("{body:?}")
    };
    assert!(text.len() <= 1 << 20);
    assert_eq!(*size, 2 << 20);
    assert!(*truncated);
    let h = SendHistory::in_memory();
    let result = send_result(
        None,
        "big",
        "POST",
        "http://h",
        &outcome("http://h", &[], body),
        None,
    );
    let rec = SendRecord::masked(header("/a", 6_000_000), result, &[]);
    h.save(&rec).unwrap();
    assert_eq!(h.load(&rec.header.id).unwrap(), rec);
}

#[test]
fn clear_one_collection_keeps_others() {
    let dir = tempfile::tempdir().unwrap();
    let h = SendHistory::at(dir.path());
    h.save(&record("/a", 1_000_000)).unwrap();
    h.save(&record("/a", 2_000_000)).unwrap();
    let b = record("/b", 3_000_000);
    h.save(&b).unwrap();
    assert_eq!(h.clear(Some(&CollectionId::of_canonical("/a"))).unwrap(), 2);
    assert_eq!(ids(&h, None), std::slice::from_ref(&b.header.id));
    assert_eq!(h.clear(None).unwrap(), 1);
    assert!(ids(&h, None).is_empty());
}

#[test]
fn unreadable_meta_is_skipped_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let h = SendHistory::at(dir.path());
    let good = record("/a", 1_000_000);
    h.save(&good).unwrap();
    let bad = RunId::new(UNIX_EPOCH + Duration::from_millis(9_000_000));
    std::fs::write(
        dir.path().join("sends").join(format!("{bad}.meta.json")),
        "{not json",
    )
    .unwrap();
    std::fs::write(dir.path().join("sends").join("notes.txt"), "x").unwrap();
    assert_eq!(ids(&h, None), std::slice::from_ref(&good.header.id));
    assert!(h.load(&bad).is_err());
}

#[test]
fn send_result_status_follows_the_outcome() {
    let ok = outcome("http://h", &[], BodyDetail::None);
    assert_eq!(
        send_result(None, "n", "GET", "u", &ok, None).status,
        ResultStatus::Pass
    );
    let failed = RequestOutcome {
        tests: vec![plover_core::script::TestResult {
            name: "t".into(),
            passed: false,
            error: Some("no".into()),
        }],
        ..ok.clone()
    };
    assert_eq!(
        send_result(None, "n", "GET", "u", &failed, None).status,
        ResultStatus::Fail
    );
    let error = send_result(
        None,
        "n",
        "GET",
        "u",
        &RequestOutcome::default(),
        Some("connection refused".into()),
    );
    assert_eq!(error.status, ResultStatus::Error);
    assert_eq!(error.error.as_deref(), Some("connection refused"));
    assert_eq!(error.file, "");
}
