//! JSON report shaped like Bruno CLI's `--reporter-json` (`{ summary, results }`), minus
//! the request and response bodies, which runs do not keep.

use serde_json::{Value, json};

use super::{RecordStatus, RequestRecord};

pub fn render(records: &[RequestRecord]) -> Value {
    let count = |status: RecordStatus| records.iter().filter(|r| r.status == status).count();
    let tests = records.iter().flat_map(|r| &r.outcome.tests);
    let asserts = records.iter().flat_map(|r| &r.outcome.asserts);
    let summary = json!({
        "totalRequests": records.len(),
        "passedRequests": count(RecordStatus::Pass),
        "failedRequests": count(RecordStatus::Fail),
        "errorRequests": count(RecordStatus::Error),
        "skippedRequests": count(RecordStatus::Skipped),
        "totalAssertions": asserts.clone().count(),
        "passedAssertions": asserts.clone().filter(|a| a.passed).count(),
        "failedAssertions": asserts.filter(|a| !a.passed).count(),
        "totalTests": tests.clone().count(),
        "passedTests": tests.clone().filter(|t| t.passed).count(),
        "failedTests": tests.filter(|t| !t.passed).count(),
    });
    let results: Vec<Value> = records
        .iter()
        .map(|r| {
            let pass = |passed: bool| if passed { "pass" } else { "fail" };
            json!({
                "test": { "filename": r.file },
                "name": r.name,
                "path": r.file.strip_suffix(".bru").unwrap_or(&r.file),
                "request": { "method": r.method, "url": r.url },
                "response": {
                    "status": r.outcome.status.map(Value::from).unwrap_or_else(|| Value::from(r.status.as_str())),
                    "responseTime": r.outcome.elapsed.as_millis() as u64,
                    "size": r.outcome.size,
                },
                "error": r.error,
                "status": r.status.as_str(),
                "skipReason": r.skip_reason,
                "assertionResults": r.outcome.asserts.iter().map(|a| {
                    let operator = a.operator.clone();
                    json!({
                        "lhsExpr": a.lhs,
                        "rhsExpr": a.rhs,
                        "operator": operator,
                        "status": pass(a.passed),
                        "error": a.error,
                    })
                }).collect::<Vec<_>>(),
                "testResults": r.outcome.tests.iter().map(|t| json!({
                    "description": t.name,
                    "status": pass(t.passed),
                    "error": t.error,
                })).collect::<Vec<_>>(),
                "runDuration": r.duration().as_secs_f64(),
            })
        })
        .collect();
    json!({ "summary": summary, "results": results })
}
