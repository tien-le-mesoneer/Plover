use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use plover_core::masking::{Masker, secret_strings};
use plover_core::run_record::{
    BodyDetail, RequestDetail, ResponseDetail, RunEnd, RunHeader, RunId, RunRecorder, RunSource,
};
use plover_core::runner::{RequestOutcome, RunEvent, RunReport, ScriptFailure, ScriptPhase};
use plover_core::script::{AssertResult, ConsoleLine, LogLevel, ScriptError, TestResult};
use plover_core::vars::ValueMap;
use serde_json::json;

const ROOT: &str = "/c";

fn text(t: &str) -> BodyDetail {
    BodyDetail::Text {
        text: t.to_owned(),
        size: t.len(),
        truncated: false,
    }
}

struct Exchange<'a> {
    url: &'a str,
    request_headers: Vec<(&'a str, &'a str)>,
    request_body: BodyDetail,
    response_headers: Vec<(&'a str, &'a str)>,
    response_body: BodyDetail,
}

impl Default for Exchange<'_> {
    fn default() -> Self {
        Exchange {
            url: "http://h/x",
            request_headers: Vec::new(),
            request_body: BodyDetail::None,
            response_headers: Vec::new(),
            response_body: BodyDetail::None,
        }
    }
}

/// The stored text of a body (sizes keep the original length).
fn body_text(body: &BodyDetail) -> &str {
    match body {
        BodyDetail::Text { text, .. } => text,
        other => panic!("{other:?}"),
    }
}

fn pairs(h: &[(&str, &str)]) -> Vec<(String, String)> {
    h.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn outcome(x: Exchange<'_>) -> RequestOutcome {
    RequestOutcome {
        status: Some(200),
        request: Some(RequestDetail {
            method: "GET".into(),
            url: x.url.into(),
            headers: pairs(&x.request_headers),
            body: x.request_body,
        }),
        response: Some(ResponseDetail {
            status: 200,
            status_text: "OK".into(),
            headers: pairs(&x.response_headers),
            body: x.response_body,
            url: x.url.into(),
            size: 0,
            size_complete: true,
        }),
        ..RequestOutcome::default()
    }
}

fn recorder(secrets: &[&str]) -> RunRecorder {
    RunRecorder::new(
        Path::new(ROOT),
        RunHeader {
            id: RunId::new(UNIX_EPOCH),
            source: RunSource::App,
            collection_root: ROOT.into(),
            collection_name: "c".into(),
            target: ".".into(),
            env: None,
            started_at_ms: 0,
        },
        secrets.iter().map(|s| s.to_string()).collect(),
    )
}

fn push(r: &mut RunRecorder, outcome: RequestOutcome) {
    let url = outcome.request.as_ref().unwrap().url.clone();
    r.event(&RunEvent::Started {
        path: Path::new(ROOT).join("a.bru"),
        name: "a".into(),
        method: "GET".into(),
        url,
    });
    r.event(&RunEvent::Passed {
        path: Path::new(ROOT).join("a.bru"),
        outcome,
    });
}

fn finished() -> RunEnd {
    RunEnd::Finished {
        report: RunReport::default(),
        end_secrets: Vec::new(),
    }
}

#[test]
fn masks_each_header_whole() {
    let mut r = recorder(&[]);
    push(
        &mut r,
        outcome(Exchange {
            request_headers: vec![
                ("AUTHORIZATION", "Basic dXNlcjpwYXNz"),
                ("Proxy-Authorization", "Bearer p"),
                ("cookie", "a=1"),
                ("X-Other", "visible"),
            ],
            response_headers: vec![("Set-Cookie", "s=1; Path=/")],
            ..Exchange::default()
        }),
    );
    let record = r.finish(finished());
    let o = &record.results[0].outcome;
    let req = &o.request.as_ref().unwrap().headers;
    assert!(req[..3].iter().all(|(_, v)| v == "***"), "{req:?}");
    assert_eq!(req[3].1, "visible");
    assert_eq!(o.response.as_ref().unwrap().headers[0].1, "***");
}

#[test]
fn bearer_token_from_login_body_is_masked() {
    let mut r = recorder(&[]);
    push(
        &mut r,
        outcome(Exchange {
            response_body: text(r#"{"access_token":"tok-AAAABBBBCCCC"}"#),
            ..Exchange::default()
        }),
    );
    push(
        &mut r,
        outcome(Exchange {
            request_headers: vec![("Authorization", "Bearer tok-AAAABBBBCCCC")],
            ..Exchange::default()
        }),
    );
    let record = r.finish(finished());
    assert_eq!(
        body_text(&record.results[0].outcome.response.as_ref().unwrap().body),
        r#"{"access_token":"***"}"#
    );
}

#[test]
fn cookie_values_masked_but_not_attributes() {
    let mut r = recorder(&[]);
    push(
        &mut r,
        outcome(Exchange {
            response_headers: vec![("Set-Cookie", "sid=cookieVALUE123; Path=/; HttpOnly")],
            response_body: text("Path=/ stays"),
            ..Exchange::default()
        }),
    );
    push(
        &mut r,
        outcome(Exchange {
            url: "http://h/x?s=cookieVALUE123",
            ..Exchange::default()
        }),
    );
    let record = r.finish(finished());
    assert_eq!(record.results[1].url, "http://h/x?s=***");
    assert_eq!(
        record.results[1].outcome.request.as_ref().unwrap().url,
        "http://h/x?s=***"
    );
    assert_eq!(
        record.results[0].outcome.response.as_ref().unwrap().body,
        text("Path=/ stays")
    );
}

#[test]
fn short_header_values_left_alone() {
    let mut r = recorder(&[]);
    push(
        &mut r,
        outcome(Exchange {
            request_headers: vec![("Authorization", "Bearer abc1234")],
            response_body: text("abc1234"),
            ..Exchange::default()
        }),
    );
    let record = r.finish(finished());
    let o = &record.results[0].outcome;
    assert_eq!(o.request.as_ref().unwrap().headers[0].1, "***");
    assert_eq!(o.response.as_ref().unwrap().body, text("abc1234"));
}

#[test]
fn secret_masked_everywhere() {
    let s = "s3cr3t-value";
    let mut r = recorder(&[s]);
    let mut o = outcome(Exchange {
        url: "http://h/x?k=s3cr3t-value",
        request_headers: vec![("X-Key", s)],
        request_body: text("body s3cr3t-value"),
        response_body: text("{\"k\":\"s3cr3t-value\"}"),
        ..Exchange::default()
    });
    o.console = vec![ConsoleLine {
        level: LogLevel::Log,
        text: "log s3cr3t-value".into(),
    }];
    o.tests = vec![TestResult {
        name: "t".into(),
        passed: false,
        error: Some("expected s3cr3t-value".into()),
    }];
    o.asserts = vec![AssertResult {
        lhs: "res.body.k".into(),
        rhs: "eq s3cr3t-value".into(),
        operator: "eq".into(),
        passed: false,
        error: Some("got s3cr3t-value".into()),
    }];
    o.script_error = Some(ScriptFailure {
        phase: ScriptPhase::PostResponse,
        error: ScriptError::Thrown {
            name: "Error".into(),
            message: "bad s3cr3t-value".into(),
            scope: None,
            line: None,
            column: None,
            stack: None,
        },
    });
    push(&mut r, o);
    let record = r.finish(finished());
    let json = serde_json::to_string(&record).unwrap();
    assert!(!json.contains(s), "{json}");
    assert!(json.contains("***"));
}

#[test]
fn secret_floor_is_four_chars() {
    let masker = Masker::new(&["abc".into(), "abcd".into()], &[]);
    assert_eq!(masker.mask_text("abc abcd"), "abc ***");
}

#[test]
fn longest_first() {
    let masker = Masker::new(&["abcdefgh".into(), "abcdefghijkl".into()], &[]);
    assert_eq!(masker.mask_text("abcdefghijkl abcdefgh"), "*** ***");
}

#[test]
fn secret_changed_by_script_is_masked() {
    let mut r = recorder(&["old-secret-1"]);
    push(
        &mut r,
        outcome(Exchange {
            response_body: text("old-secret-1 new-secret-2"),
            ..Exchange::default()
        }),
    );
    let record = r.finish(RunEnd::Finished {
        report: RunReport::default(),
        end_secrets: vec!["new-secret-2".into()],
    });
    assert_eq!(
        body_text(&record.results[0].outcome.response.as_ref().unwrap().body),
        "*** ***"
    );
}

#[test]
fn base64_bodies_not_searched() {
    let body = BodyDetail::Base64 {
        data: "s3cr3t-value".into(),
        size: 9,
        truncated: false,
    };
    let mut r = recorder(&["s3cr3t-value"]);
    push(
        &mut r,
        outcome(Exchange {
            response_body: body.clone(),
            ..Exchange::default()
        }),
    );
    let record = r.finish(finished());
    assert_eq!(
        record.results[0].outcome.response.as_ref().unwrap().body,
        body
    );
}

#[test]
fn live_detail_uses_values_so_far() {
    let mut r = recorder(&[]);
    push(
        &mut r,
        outcome(Exchange {
            response_body: text("tok-AAAABBBBCCCC"),
            ..Exchange::default()
        }),
    );
    let live = r.masked_result(0).unwrap();
    assert_eq!(
        live.outcome.response.unwrap().body,
        text("tok-AAAABBBBCCCC")
    );
    push(
        &mut r,
        outcome(Exchange {
            request_headers: vec![("Authorization", "Bearer tok-AAAABBBBCCCC")],
            ..Exchange::default()
        }),
    );
    let live = r.masked_result(0).unwrap();
    assert_eq!(body_text(&live.outcome.response.unwrap().body), "***");
    assert!(r.masked_result(9).is_none());
    let record = r.finish(finished());
    assert_eq!(
        body_text(&record.results[0].outcome.response.as_ref().unwrap().body),
        "***"
    );
}

#[test]
fn cancelled_run_counts_its_results() {
    let mut r = recorder(&[]);
    push(&mut r, outcome(Exchange::default()));
    let record = r.finish(RunEnd::Cancelled {
        end_secrets: Vec::new(),
    });
    assert_eq!(record.outcome.as_str(), "cancelled");
    assert_eq!(record.report.passed, 1);
    assert!(record.report.elapsed < Duration::from_secs(60));
}

#[test]
fn secret_strings_takes_scalars() {
    let mut values = ValueMap::new();
    values.insert("a".into(), json!("text"));
    values.insert("b".into(), json!(12345));
    values.insert("c".into(), json!({"x": 1}));
    values.insert("d".into(), json!(true));
    let mut got = secret_strings(&values);
    got.sort();
    assert_eq!(got, ["12345", "text", "true"]);
}
