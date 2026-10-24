//! JSON report shaped like Bruno CLI's `--reporter-json`: a list of iterations, each
//! `{ iterationIndex, summary, results }`, with the request and response of each result
//! (run-single-request.js) after masking and the `--reporter-skip-*` flags.

use serde_json::{Map, Value, json};

use super::ReportFilter;
use crate::run_record::{
    BodyDetail, POST_RESPONSE_SCRIPT_ERROR, PartDetail, ResultRecord, ResultStatus, RunRecord,
};

pub fn render(record: &RunRecord, filter: &ReportFilter) -> Value {
    let entries: Vec<Value> = super::iterations(record)
        .into_iter()
        .map(|(index, results)| {
            let results: Vec<Value> = results.iter().map(|r| result(r, filter)).collect();
            json!({ "iterationIndex": index, "summary": summary(&results_of(record, index)), "results": results })
        })
        .collect();
    Value::Array(entries)
}

fn results_of(record: &RunRecord, index: usize) -> Vec<&ResultRecord> {
    record
        .results
        .iter()
        .filter(|r| r.iteration_index == index)
        .collect()
}

/// Bruno's `getRunnerSummary` for one iteration.
fn summary(results: &[&ResultRecord]) -> Value {
    let count = |status: ResultStatus| results.iter().filter(|r| r.status == status).count();
    let tests = results.iter().flat_map(|r| &r.outcome.tests);
    let asserts = results.iter().flat_map(|r| &r.outcome.asserts);
    json!({
        "totalRequests": results.len(),
        "passedRequests": count(ResultStatus::Pass),
        "failedRequests": count(ResultStatus::Fail),
        "errorRequests": count(ResultStatus::Error),
        "skippedRequests": count(ResultStatus::Skipped),
        "totalAssertions": asserts.clone().count(),
        "passedAssertions": asserts.clone().filter(|a| a.passed).count(),
        "failedAssertions": asserts.filter(|a| !a.passed).count(),
        "totalTests": tests.clone().count(),
        "passedTests": tests.clone().filter(|t| t.passed).count(),
        "failedTests": tests.filter(|t| !t.passed).count(),
    })
}

fn result(r: &ResultRecord, filter: &ReportFilter) -> Value {
    let pass = |passed: bool| if passed { "pass" } else { "fail" };
    // Bruno's per-result status says whether the request completed; failed tests and
    // asserts show in their own lists and in the summary.
    let status = match r.status {
        ResultStatus::Pass | ResultStatus::Fail => "pass",
        other => other.as_str(),
    };
    let post_response_tests: Vec<Value> = r
        .post_response_script_error()
        .map(|message| json!({ "description": POST_RESPONSE_SCRIPT_ERROR, "status": "fail", "error": message }))
        .into_iter()
        .collect();
    let mut request = Map::new();
    let sent = r.outcome.request.as_ref();
    request.insert(
        "method".into(),
        sent.map_or(r.method.as_str(), |d| d.method.as_str()).into(),
    );
    request.insert(
        "url".into(),
        sent.map_or(r.url.as_str(), |d| d.url.as_str()).into(),
    );
    request.insert(
        "headers".into(),
        headers(
            sent.map(|d| d.headers.as_slice()).unwrap_or_default(),
            filter,
        ),
    );
    if !filter.skip_request_body {
        request.insert(
            "data".into(),
            sent.map_or(Value::Null, |d| request_data(&d.body)),
        );
    }
    let mut response = Map::new();
    let received = r.outcome.response.as_ref();
    let response_time = r.outcome.elapsed.as_millis() as u64;
    response.insert(
        "status".into(),
        r.outcome
            .status
            .map(Value::from)
            .unwrap_or_else(|| Value::from(r.status.as_str())),
    );
    response.insert(
        "statusText".into(),
        received.map_or(Value::Null, |d| d.status_text.clone().into()),
    );
    response.insert(
        "headers".into(),
        headers(
            received.map(|d| d.headers.as_slice()).unwrap_or_default(),
            filter,
        ),
    );
    if !filter.skip_response_body {
        response.insert(
            "data".into(),
            received.map_or(Value::Null, |d| response_data(&d.body)),
        );
    }
    response.insert(
        "url".into(),
        received.map_or(Value::Null, |d| d.url.clone().into()),
    );
    response.insert("responseTime".into(), response_time.into());
    response.insert("duration".into(), response_time.into());
    response.insert("size".into(), r.outcome.size.into());
    json!({
        "test": { "filename": r.file },
        "name": r.name,
        "path": r.file.strip_suffix(".bru").unwrap_or(&r.file),
        "request": request,
        "response": response,
        "error": if r.status == ResultStatus::Error { r.error.clone() } else { None },
        "status": status,
        "skipReason": r.skip_reason,
        "assertionResults": r.outcome.asserts.iter().map(|a| json!({
            "lhsExpr": a.lhs,
            "rhsExpr": a.rhs,
            "operator": a.operator,
            "status": pass(a.passed),
            "error": a.error,
        })).collect::<Vec<_>>(),
        "testResults": r.outcome.tests.iter().map(|t| json!({
            "description": t.name,
            "status": pass(t.passed),
            "error": t.error,
        })).collect::<Vec<_>>(),
        "postResponseTestResults": post_response_tests,
        "runDuration": r.duration().as_secs_f64(),
        "iterationIndex": r.iteration_index,
    })
}

/// Bruno's headers object; a repeated name becomes an array of its values.
fn headers(headers: &[(String, String)], filter: &ReportFilter) -> Value {
    let mut out = Map::new();
    for (name, value) in filter.headers(headers) {
        match out.get_mut(name) {
            None => {
                out.insert(name.clone(), value.clone().into());
            }
            Some(Value::Array(values)) => values.push(value.clone().into()),
            Some(first) => *first = Value::Array(vec![first.take(), value.clone().into()]),
        }
    }
    Value::Object(out)
}

fn request_data(body: &BodyDetail) -> Value {
    match body {
        BodyDetail::None | BodyDetail::Omitted { .. } => Value::Null,
        BodyDetail::Text { text, .. } => text.clone().into(),
        BodyDetail::Base64 { data, .. } => data.clone().into(),
        BodyDetail::Form { fields } => Value::Object(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), Value::from(v.clone())))
                .collect(),
        ),
        BodyDetail::Multipart { parts } => parts
            .iter()
            .map(|p| match p {
                PartDetail::Text { name, value, .. } => json!({ "name": name, "value": value }),
                PartDetail::File {
                    name, file_name, ..
                } => json!({ "name": name, "fileName": file_name }),
            })
            .collect(),
    }
}

/// Bruno's `parseDataFromResponse`: the body as JSON whenever it parses (BOM stripped),
/// else the text.
fn response_data(body: &BodyDetail) -> Value {
    match body {
        BodyDetail::None => "".into(),
        BodyDetail::Text {
            text,
            truncated: false,
            ..
        } => {
            let text = text.strip_prefix('\u{feff}').unwrap_or(text);
            serde_json::from_str(text).unwrap_or_else(|_| text.into())
        }
        BodyDetail::Text { text, .. } => text.clone().into(),
        other => request_data(other),
    }
}
