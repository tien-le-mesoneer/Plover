//! Reports rendered from a run record: Bruno's JSON shape, masking, skip flags, HTML detail.

use std::time::Duration;

use plover_core::report::{ReportFilter, html, json, junit};
use plover_core::run_record::{
    BodyDetail, RUN_RECORD_VERSION, RequestDetail, ResponseDetail, ResultRecord, ResultStatus,
    RunHeader, RunId, RunOutcome, RunRecord, RunSource,
};
use plover_core::runner::{RequestOutcome, RunReport};

fn text(t: &str) -> BodyDetail {
    BodyDetail::Text {
        text: t.to_owned(),
        size: t.len(),
        truncated: false,
    }
}

fn record() -> RunRecord {
    let sent = ResultRecord {
        index: 0,
        file: "a.bru".into(),
        name: "a".into(),
        method: "POST".into(),
        url: "http://h/a".into(),
        status: ResultStatus::Pass,
        outcome: RequestOutcome {
            status: Some(200),
            elapsed: Duration::from_millis(7),
            size: 8,
            request: Some(RequestDetail {
                method: "POST".into(),
                url: "http://h/a".into(),
                headers: vec![
                    ("Authorization".into(), "***".into()),
                    ("x-r".into(), "1".into()),
                    ("x-r".into(), "2".into()),
                ],
                body: text("{\"q\":1}"),
            }),
            response: Some(ResponseDetail {
                status: 200,
                status_text: "OK".into(),
                headers: vec![("X-R".into(), "y".into()), ("x-keep".into(), "k".into())],
                body: text("\u{feff}{\"id\":1}"),
                url: "http://h/a".into(),
                size: 8,
                size_complete: true,
            }),
            ..RequestOutcome::default()
        },
        error: None,
        skip_reason: None,
    };
    let errored = ResultRecord {
        index: 1,
        file: "b.bru".into(),
        name: "b".into(),
        method: "GET".into(),
        url: "http://127.0.0.1:9/b".into(),
        status: ResultStatus::Error,
        outcome: RequestOutcome {
            request: Some(RequestDetail {
                method: "GET".into(),
                url: "http://127.0.0.1:9/b".into(),
                headers: Vec::new(),
                body: BodyDetail::None,
            }),
            ..RequestOutcome::default()
        },
        error: Some("connection refused".into()),
        skip_reason: None,
    };
    let text_body = ResultRecord {
        index: 2,
        file: "c.bru".into(),
        name: "<c>".into(),
        status: ResultStatus::Pass,
        outcome: RequestOutcome {
            status: Some(200),
            request: Some(RequestDetail {
                method: "GET".into(),
                url: "http://h/c".into(),
                headers: Vec::new(),
                body: BodyDetail::None,
            }),
            response: Some(ResponseDetail {
                status: 200,
                status_text: "OK".into(),
                headers: Vec::new(),
                body: text("<b>not json</b>"),
                url: "http://h/c".into(),
                size: 15,
                size_complete: true,
            }),
            ..RequestOutcome::default()
        },
        ..errored.clone()
    };
    RunRecord {
        version: RUN_RECORD_VERSION,
        header: RunHeader {
            id: RunId::new(std::time::UNIX_EPOCH),
            source: RunSource::Plv,
            collection_root: "/c".into(),
            collection_name: "My <API>".into(),
            target: ".".into(),
            env: Some("local".into()),
            started_at_ms: 1_761_134_400_123,
        },
        outcome: RunOutcome::Completed,
        report: RunReport {
            passed: 2,
            failed: 1,
            ..RunReport::default()
        },
        results: vec![sent, errored, text_body],
    }
}

#[test]
fn json_report_has_bruno_request_and_response() {
    let v = json::render(&record(), &ReportFilter::default());
    let results = v[0]["results"].as_array().unwrap();
    let a = &results[0];
    assert_eq!(a["request"]["method"], "POST");
    assert_eq!(a["request"]["url"], "http://h/a");
    assert_eq!(a["request"]["headers"]["Authorization"], "***");
    assert_eq!(
        a["request"]["headers"]["x-r"],
        serde_json::json!(["1", "2"])
    );
    assert_eq!(a["request"]["data"], "{\"q\":1}");
    let res = &a["response"];
    assert_eq!(res["status"], 200);
    assert_eq!(res["statusText"], "OK");
    assert_eq!(res["headers"]["X-R"], "y");
    assert_eq!(res["data"], serde_json::json!({"id": 1}));
    assert_eq!(res["url"], "http://h/a");
    assert_eq!(
        (res["responseTime"].clone(), res["duration"].clone()),
        (7.into(), 7.into())
    );
    assert_eq!(res["size"], 8);
    let b = &results[1]["response"];
    assert_eq!(b["status"], "error");
    assert_eq!(b["size"], 0);
    assert!(b["data"].is_null());
    assert_eq!(results[2]["response"]["data"], "<b>not json</b>");
}

#[test]
fn skip_flags_match_bruno() {
    let all = json::render(
        &record(),
        &ReportFilter {
            skip_all_headers: true,
            ..ReportFilter::default()
        },
    );
    let a = &all[0]["results"][0];
    assert_eq!(a["request"]["headers"], serde_json::json!({}));
    assert_eq!(a["response"]["headers"], serde_json::json!({}));
    let some = json::render(
        &record(),
        &ReportFilter {
            skip_headers: vec!["X-r".into()],
            ..ReportFilter::default()
        },
    );
    let a = &some[0]["results"][0];
    assert!(a["request"]["headers"].get("x-r").is_none());
    assert!(a["response"]["headers"].get("X-R").is_none());
    assert_eq!(a["response"]["headers"]["x-keep"], "k");
    let bodies = json::render(
        &record(),
        &ReportFilter {
            skip_request_body: true,
            skip_response_body: true,
            ..ReportFilter::default()
        },
    );
    let a = &bodies[0]["results"][0];
    assert!(a["request"].get("data").is_none());
    assert!(a["response"].get("data").is_none());
    let html = html::render(
        &record(),
        &ReportFilter {
            skip_all_headers: true,
            skip_request_body: true,
            skip_response_body: true,
            ..ReportFilter::default()
        },
    );
    assert!(!html.contains("x-keep"));
    assert!(!html.contains("{&quot;id&quot;:1}"));
}

#[test]
fn html_has_details_and_no_script() {
    let html = html::render(&record(), &ReportFilter::default());
    assert_eq!(html.matches("<details>").count(), 3);
    assert!(html.contains("<code>x-keep</code>"));
    assert!(html.contains("{&quot;id&quot;:1}"));
    assert!(html.contains("&lt;b&gt;not json&lt;/b&gt;"));
    assert!(html.contains("My &lt;API&gt; · local · 2025-10-22T12:00:00.123 UTC"));
    assert!(html.contains("No response"));
    for forbidden in ["<script", "<link", "src=\"http", "href=\"http", "@import"] {
        assert!(!html.contains(forbidden), "found {forbidden}");
    }
}

#[test]
fn junit_uses_run_start_time() {
    let xml = junit::render(&record());
    assert!(
        xml.contains("timestamp=\"2025-10-22T12:00:00.123\""),
        "{xml}"
    );
    assert_eq!(xml.matches("<testsuite ").count(), 3);
}
