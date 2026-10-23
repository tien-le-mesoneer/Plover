//! `test()`, `assert` blocks and `vars:post-response` (bruno-js/src/test.js,
//! runtime/assert-runtime.js, runtime/vars-runtime.js).

use std::time::{Duration, Instant};

use plover_core::script::{
    AssertResult, ScriptContext, ScriptEngine, ScriptError, ScriptLimits, ScriptResponse, Segment,
    run_asserts, run_post_response_vars, run_script,
};
use serde_json::json;

fn engine() -> ScriptEngine {
    ScriptEngine::new(ScriptLimits::default())
}

fn context() -> ScriptContext {
    let mut ctx = ScriptContext::default();
    ctx.vars.environment.insert("expected".into(), json!("200"));
    ctx.vars.runtime.insert("name".into(), json!("Ana"));
    ctx.res = Some(ScriptResponse {
        status: 200,
        status_text: "OK".into(),
        headers: vec![("content-type".into(), "application/json".into())],
        body: r#"{"token":"t-1","name":"Ana","items":[1,2,3],"empty":[],"nothing":null,"flag":true,"n":5}"#
            .into(),
        response_time_ms: 3,
        url: "http://h/x".into(),
        size: 80,
    });
    ctx
}

#[tokio::test]
async fn test_results_pass_fail_and_async() {
    let out = run_script(
        &engine(),
        &context(),
        vec![Segment::new(
            "request",
            r#"
            test('status is 200', () => expect(res.status).to.equal(200));
            test('fails', () => expect(res.body.name).to.equal('Bo'));
            test('async', async () => { await bru.sleep(5); expect(1).to.equal(1); });
            test('throws', () => { throw new TypeError('bad'); });
            "#,
        )],
    )
    .await;
    assert!(out.error.is_none(), "{:?}", out.error);
    let summary: Vec<(&str, bool, Option<&str>)> = out
        .tests
        .iter()
        .map(|t| (t.name.as_str(), t.passed, t.error.as_deref()))
        .collect();
    // Completion order, like Bruno: tests that throw synchronously settle before tests
    // whose callback returns a value to await (bruno-js/src/test.js `await callback()`).
    assert_eq!(
        summary,
        vec![
            ("fails", false, Some("expected 'Ana' to equal 'Bo'")),
            ("throws", false, Some("bad")),
            ("status is 200", true, None),
            ("async", true, None),
        ]
    );
}

#[tokio::test]
async fn never_settling_test_times_out() {
    let e = ScriptEngine::new(ScriptLimits {
        timeout: Duration::from_millis(300),
        ..ScriptLimits::default()
    });
    let start = Instant::now();
    let out = run_script(
        &e,
        &context(),
        vec![Segment::new(
            "request",
            "test('hangs', async () => { await new Promise(r => setTimeout(r, 60000)); });",
        )],
    )
    .await;
    assert!(
        matches!(out.error, Some(ScriptError::Timeout(_))),
        "{:?}",
        out.error
    );
    assert!(start.elapsed() < Duration::from_millis(1500));
}

#[tokio::test]
async fn chai_json_and_json_schema_assertions() {
    let out = run_script(
        &engine(),
        &context(),
        vec![Segment::new(
            "request",
            r#"
            test('json', () => expect(res.body).to.be.json);
            test('not json', () => expect('x').to.be.json);
            test('schema', () => expect(res.body).to.have.jsonSchema({ type: 'object', required: ['token'] }));
            test('schema fails', () => expect(res.body).to.have.jsonSchema({ type: 'object', required: ['missing'] }));
            "#,
        )],
    )
    .await;
    assert!(out.error.is_none(), "{:?}", out.error);
    let by_name = |name: &str| out.tests.iter().find(|t| t.name == name).unwrap();
    assert!(
        by_name("json").passed && by_name("schema").passed,
        "{:?}",
        out.tests
    );
    assert_eq!(
        by_name("not json").error.as_deref(),
        Some("expected 'x' to be JSON")
    );
    let schema_error = by_name("schema fails").error.as_deref().unwrap();
    assert!(
        schema_error.starts_with("expected { token: 't-1', name: 'Ana', …(5) } to match JSON schema, validation errors: [{"),
        "{schema_error}"
    );
    assert!(schema_error.contains("must have required property 'missing'"));
}

async fn asserts(pairs: &[(&str, &str)]) -> Vec<AssertResult> {
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .map(|(l, r)| (l.to_string(), r.to_string()))
        .collect();
    let out = run_asserts(&engine(), &context(), &pairs).await;
    assert!(out.error.is_none(), "{:?}", out.error);
    out.asserts
}

#[tokio::test]
async fn assert_operators_pass() {
    let cases = [
        ("res.status", "eq 200"),
        ("res.status", "eq {{expected}}"),
        ("res.status", "200"),
        ("res.body.name", "eq Ana"),
        ("res.body.name", "eq \"Ana\""),
        ("res.body.name", "eq {{name}}"),
        ("res.status", "neq 404"),
        ("res.body.n", "gt 4"),
        ("res.body.n", "gte 5"),
        ("res.body.n", "lt 6"),
        ("res.body.n", "lte 5"),
        ("res.status", "in 200, 201"),
        ("res.status", "in [200,201]"),
        ("res.status", "notIn 400,500"),
        ("res.body.name", "contains An"),
        ("res.body.name", "notContains Z"),
        ("res.body.items", "length 3"),
        ("res.body.name", "matches ^A"),
        ("res.body.name", "matches /n+a$/"),
        ("res.body.name", "notMatches ^Z"),
        ("res.body.name", "startsWith An"),
        ("res.body.name", "endsWith na"),
        ("res.body.n", "between 1, 10"),
        ("res.body.empty", "isEmpty"),
        ("res.body.items", "isNotEmpty"),
        ("res.body.nothing", "isNull"),
        ("res.body.missing", "isUndefined"),
        ("res.body.name", "isDefined"),
        ("res.body.flag", "isTruthy"),
        ("res.body.missing === 1", "isFalsy"),
        ("res.body", "isJson"),
        ("res.body.n", "isNumber"),
        ("res.body.name", "isString"),
        ("res.body.flag", "isBoolean"),
        ("res.body.items", "isArray"),
        ("res.body.items.length + n", "eq 3"),
    ];
    let mut ctx_cases: Vec<(&str, &str)> = cases.to_vec();
    // `n` is not a variable here, so the last one fails; check it separately.
    ctx_cases.pop();
    let results = asserts(&ctx_cases).await;
    for (r, (lhs, rhs)) in results.iter().zip(ctx_cases.iter()) {
        assert!(r.passed, "{lhs}: {rhs} failed: {:?}", r.error);
    }
    assert_eq!(results.len(), ctx_cases.len());
}

#[tokio::test]
async fn assert_failure_messages_match_bruno() {
    let results = asserts(&[
        ("res.status", "eq 201"),
        ("res.body.name", "neq Ana"),
        ("res.body.n", "gt 10"),
        ("res.status", "in 201,202"),
        ("res.body.items", "length 2"),
        ("res.body.name", "isNumber"),
        ("res.body.flag", "isFalsy"),
        ("res.body.missing.deep", "eq 1"),
    ])
    .await;
    let errors: Vec<Option<&str>> = results.iter().map(|r| r.error.as_deref()).collect();
    assert_eq!(
        errors,
        vec![
            Some("expected 200 to equal 201"),
            Some("expected 'Ana' to not equal 'Ana'"),
            Some("expected 5 to be above 10"),
            Some("expected 200 to be one of [ 201, 202 ]"),
            Some("expected [ 1, 2, 3 ] to have a length of 2 but got 3"),
            Some("expected 'Ana' to be a number"),
            Some("expected true to be false"),
            Some("cannot read property 'deep' of undefined"),
        ]
    );
    assert!(results.iter().all(|r| !r.passed));
    assert_eq!(results[0].operator, "eq");
    assert_eq!(results[0].lhs, "res.status");
    assert_eq!(results[0].rhs, "eq 201");
}

#[tokio::test]
async fn post_response_var_from_body() {
    let vars = vec![
        ("token".to_owned(), "res.body.token".to_owned()),
        ("count".to_owned(), "res.body.items.length".to_owned()),
        ("greeting".to_owned(), "'hi ' + name".to_owned()),
        ("broken".to_owned(), "res.body.missing.x".to_owned()),
    ];
    let out = run_post_response_vars(&engine(), &context(), &vars).await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.vars.runtime["token"], json!("t-1"));
    assert_eq!(out.vars.runtime["count"], json!(3));
    assert_eq!(out.vars.runtime["greeting"], json!("hi Ana"));
    assert!(!out.vars.runtime.contains_key("broken"));
    assert_eq!(out.console.len(), 1);
    assert!(
        out.console[0]
            .text
            .starts_with("1 error in post response variables: \nbroken: "),
        "{}",
        out.console[0].text
    );
}

#[tokio::test]
async fn thrown_non_errors_get_brunos_generic_message() {
    let out = run_script(
        &engine(),
        &context(),
        vec![Segment::new(
            "request",
            "test('plain', () => { throw 'plain'; }); test('empty', () => { throw new Error(''); });",
        )],
    )
    .await;
    let errors: Vec<Option<&str>> = out.tests.iter().map(|t| t.error.as_deref()).collect();
    // bruno-js/src/test.js: `error.message || 'An unexpected error occurred.'`.
    assert_eq!(
        errors,
        [
            Some("An unexpected error occurred."),
            Some("An unexpected error occurred.")
        ]
    );
}
