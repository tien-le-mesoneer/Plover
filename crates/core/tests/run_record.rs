use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use plover_core::run_record::{
    BodyDetail, MAX_DETAIL_BODY, MAX_RUN_BODIES, PartDetail, RUN_RECORD_VERSION, RequestDetail,
    ResponseDetail, ResultRecord, ResultStatus, RunHeader, RunId, RunOutcome, RunRecord,
    RunRecorder, RunSource,
};
use plover_core::runner::{RequestOutcome, RunEvent, RunReport, ScriptFailure, ScriptPhase};
use plover_core::script::{ConsoleLine, LogLevel, ScriptError, TestResult};

const ROOT: &str = "/c";

fn header() -> RunHeader {
    RunHeader {
        id: RunId::new(UNIX_EPOCH + Duration::from_secs(86_400)),
        source: RunSource::Plv,
        collection_root: ROOT.into(),
        collection_name: "c".into(),
        target: ".".into(),
        env: Some("local".into()),
        started_at_ms: 86_400_000,
    }
}

fn recorder() -> RunRecorder {
    RunRecorder::new(Path::new(ROOT), header(), Vec::new())
}

fn started(path: &str, name: &str) -> RunEvent {
    RunEvent::Started {
        path: Path::new(ROOT).join(path),
        name: name.into(),
        method: "GET".into(),
        url: format!("http://h/{name}"),
    }
}

fn outcome(status: Option<u16>) -> RequestOutcome {
    RequestOutcome {
        status,
        ..RequestOutcome::default()
    }
}

#[test]
fn records_pass_fail_error_skipped() {
    let mut r = recorder();
    assert_eq!(r.event(&started("a.bru", "a")), None);
    let p = |s: &str| Path::new(ROOT).join(s);
    assert_eq!(
        r.event(&RunEvent::Passed {
            path: p("a.bru"),
            outcome: outcome(Some(200)),
        }),
        Some(0)
    );
    r.event(&started("dir/b.bru", "b"));
    assert_eq!(
        r.event(&RunEvent::Failed {
            path: p("dir/b.bru"),
            name: "b".into(),
            error: "1 test failed".into(),
            outcome: outcome(Some(500)),
        }),
        Some(1)
    );
    assert_eq!(
        r.event(&RunEvent::Failed {
            path: p("c.bru"),
            name: "c".into(),
            error: "connection refused".into(),
            outcome: outcome(None),
        }),
        Some(2)
    );
    assert_eq!(
        r.event(&RunEvent::Skipped {
            path: p("d.bru"),
            name: "d".into(),
            reason: "parse error".into(),
        }),
        Some(3)
    );
    let results = r.results();
    let statuses: Vec<_> = results.iter().map(|x| x.status).collect();
    assert_eq!(
        statuses,
        [
            ResultStatus::Pass,
            ResultStatus::Fail,
            ResultStatus::Error,
            ResultStatus::Skipped
        ]
    );
    let indices: Vec<_> = results.iter().map(|x| x.index).collect();
    assert_eq!(indices, [0, 1, 2, 3]);
    assert_eq!(results[1].file, "dir/b.bru");
    assert_eq!(
        (results[1].method.as_str(), results[1].url.as_str()),
        ("GET", "http://h/b")
    );
    assert_eq!(results[2].name, "c");
    assert_eq!(
        (results[2].method.as_str(), results[2].url.as_str()),
        ("", "")
    );
    assert_eq!(results[2].error.as_deref(), Some("connection refused"));
    assert_eq!(results[3].skip_reason.as_deref(), Some("parse error"));
}

#[test]
fn pre_request_error_reports_thrown_message() {
    let mut r = recorder();
    let mut o = outcome(None);
    o.script_error = Some(ScriptFailure {
        phase: ScriptPhase::PreRequest,
        error: ScriptError::Thrown {
            name: "Error".into(),
            message: "Request failed with status code 401".into(),
            scope: Some("request".into()),
            line: Some(1),
            column: None,
            stack: None,
        },
    });
    r.event(&RunEvent::Failed {
        path: Path::new(ROOT).join("a.bru"),
        name: "a".into(),
        error: "pre-request script error: Error: Request failed with status code 401".into(),
        outcome: o,
    });
    assert_eq!(
        r.results()[0].error.as_deref(),
        Some("Request failed with status code 401")
    );
}

#[test]
fn same_item_twice_gets_two_indices() {
    let mut r = recorder();
    for status in [200, 201] {
        r.event(&started("a.bru", "a"));
        r.event(&RunEvent::Passed {
            path: Path::new(ROOT).join("a.bru"),
            outcome: outcome(Some(status)),
        });
    }
    let results = r.results();
    assert_eq!(results.len(), 2);
    assert_eq!((results[0].index, results[1].index), (0, 1));
    assert_eq!(results[0].outcome.status, Some(200));
    assert_eq!(results[1].outcome.status, Some(201));
}

fn detail_outcome(body: &str) -> RequestOutcome {
    let text = |t: &str| BodyDetail::Text {
        text: t.to_owned(),
        size: t.len(),
        truncated: false,
    };
    RequestOutcome {
        status: Some(200),
        request: Some(RequestDetail {
            method: "POST".into(),
            url: "http://h/a".into(),
            headers: vec![("x-a".into(), "1".into())],
            body: text(body),
        }),
        response: Some(ResponseDetail {
            status: 200,
            status_text: "OK".into(),
            headers: vec![("x-b".into(), "2".into())],
            body: text(body),
            url: "http://h/a".into(),
            size: body.len(),
            size_complete: true,
        }),
        ..RequestOutcome::default()
    }
}

#[test]
fn run_body_budget_drops_later_bodies() {
    let mut r = recorder();
    let body = "a".repeat(MAX_DETAIL_BODY);
    // Two bodies (request + response) per result.
    let results = MAX_RUN_BODIES / (2 * MAX_DETAIL_BODY) + 2;
    for _ in 0..results {
        r.event(&started("a.bru", "a"));
        r.event(&RunEvent::Passed {
            path: Path::new(ROOT).join("a.bru"),
            outcome: detail_outcome(&body),
        });
    }
    let first = &r.results()[0].outcome;
    assert!(matches!(
        first.response.as_ref().unwrap().body,
        BodyDetail::Text { .. }
    ));
    let last = &r.results().last().unwrap().outcome;
    for body in [
        &last.request.as_ref().unwrap().body,
        &last.response.as_ref().unwrap().body,
    ] {
        assert_eq!(
            body,
            &BodyDetail::Omitted {
                reason: "run size limit".into(),
                size: MAX_DETAIL_BODY
            }
        );
    }
    assert_eq!(
        last.response.as_ref().unwrap().headers,
        [("x-b".to_owned(), "2".to_owned())]
    );
    let kept: usize = r
        .results()
        .iter()
        .flat_map(|x| {
            [
                &x.outcome.request.as_ref().unwrap().body,
                &x.outcome.response.as_ref().unwrap().body,
            ]
        })
        .map(|b| b.stored_len())
        .sum();
    assert!(kept <= MAX_RUN_BODIES);
}

#[test]
fn run_id_format() {
    let id = RunId::new(UNIX_EPOCH + Duration::from_secs(86_400));
    let s = id.as_str();
    assert!(s.starts_with("19700102T000000Z-"), "{s}");
    assert_eq!(s.len(), 23);
    assert!(
        s[17..]
            .bytes()
            .all(|c| c.is_ascii_lowercase() || (b'2'..=b'7').contains(&c))
    );
    assert_eq!(RunId::parse(s), Some(id.clone()));
    assert_eq!(RunId::parse("../x"), None);
    assert_eq!(RunId::parse("19700102T000000Z-ABCDEF"), None);
    assert_eq!(RunId::parse("19700102T000000Z-abcde1"), None);
    assert_ne!(RunId::new(SystemTime::now()), RunId::new(SystemTime::now()));
}

#[test]
fn record_round_trips_json() {
    let bodies = vec![
        BodyDetail::None,
        BodyDetail::Text {
            text: "t".into(),
            size: 1,
            truncated: false,
        },
        BodyDetail::Base64 {
            data: "/wAB".into(),
            size: 3,
            truncated: true,
        },
        BodyDetail::Form {
            fields: vec![("a".into(), "b".into())],
        },
        BodyDetail::Multipart {
            parts: vec![
                PartDetail::Text {
                    name: "n".into(),
                    value: "v".into(),
                    content_type: None,
                },
                PartDetail::File {
                    name: "f".into(),
                    file_name: "x.png".into(),
                    content_type: Some("image/png".into()),
                },
            ],
        },
        BodyDetail::Omitted {
            reason: "run size limit".into(),
            size: 9,
        },
    ];
    let results = bodies
        .into_iter()
        .enumerate()
        .map(|(index, body)| {
            let mut outcome = detail_outcome("x");
            outcome.elapsed = Duration::from_millis(12);
            outcome.duration = Duration::from_millis(15);
            outcome.request.as_mut().unwrap().body = body;
            outcome.console = vec![ConsoleLine {
                level: LogLevel::Warn,
                text: "w".into(),
            }];
            outcome.tests = vec![TestResult {
                name: "t".into(),
                passed: false,
                error: Some("e".into()),
            }];
            outcome.script_error = Some(ScriptFailure {
                phase: ScriptPhase::Tests,
                error: ScriptError::Timeout(Duration::from_millis(500)),
            });
            ResultRecord {
                index,
                file: "a.bru".into(),
                name: "a".into(),
                method: "POST".into(),
                url: "http://h/a".into(),
                status: ResultStatus::Fail,
                outcome,
                error: Some("1 test failed".into()),
                skip_reason: None,
            }
        })
        .collect();
    let record = RunRecord {
        version: RUN_RECORD_VERSION,
        header: header(),
        outcome: RunOutcome::Completed,
        report: RunReport {
            passed: 1,
            elapsed: Duration::from_millis(42),
            ..RunReport::default()
        },
        results,
    };
    let json = serde_json::to_string(&record).unwrap();
    let back: RunRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back, record);
}
