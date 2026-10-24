//! `bru.runner.iterationData`, `iterationIndex`, `totalIterations` and the row's place in
//! interpolation (Bruno CLI 4.3.0 bru.js IterationDataManager, interpolate-vars.js).

use plover_core::script::{
    IterationInfo, ScriptContext, ScriptEngine, ScriptLimits, ScriptResponse, Segment, run_asserts,
    run_script,
};
use plover_core::vars::ValueMap;
use serde_json::{Value, json};

fn engine() -> ScriptEngine {
    ScriptEngine::new(ScriptLimits::default())
}

fn map(v: Value) -> ValueMap {
    v.as_object().unwrap().clone().into_iter().collect()
}

fn ctx(row: Value, runtime: Value, iteration: Option<IterationInfo>) -> ScriptContext {
    let mut ctx = ScriptContext::default();
    ctx.vars.iteration = map(row);
    ctx.vars.runtime = map(runtime);
    ctx.info.iteration = iteration;
    ctx
}

async fn run(ctx: &ScriptContext, src: &str) -> plover_core::script::PhaseOutput {
    let out = run_script(&engine(), ctx, vec![Segment::new("request", src)]).await;
    assert!(out.error.is_none(), "{:?} {:?}", out.error, out.console);
    out
}

#[test]
fn row_beats_runtime_in_var_stack() {
    let c = ctx(json!({"x":"row"}), json!({"x":"rt"}), None);
    assert_eq!(c.vars.var_stack().interpolate("{{x}}"), "row");
}

#[tokio::test]
async fn getvar_and_interpolate_ignore_row() {
    let c = ctx(
        json!({"x":"row"}),
        json!({"x":"rt"}),
        Some(IterationInfo { index: 0, total: 1 }),
    );
    let out = run(
        &c,
        r#"bru.setVar("a", bru.getVar("x")); bru.setVar("b", bru.interpolate("{{x}}"));"#,
    )
    .await;
    assert_eq!(out.vars.runtime["a"], json!("rt"));
    assert_eq!(out.vars.runtime["b"], json!("rt"));
}

#[tokio::test]
async fn same_name_as_env_var() {
    let mut c = ctx(json!({"baseUrl":"http://row"}), json!({}), None);
    c.vars
        .environment
        .insert("baseUrl".into(), json!("http://env"));
    assert_eq!(c.vars.var_stack().interpolate("{{baseUrl}}"), "http://row");
    let out = run(&c, r#"bru.setVar("e", bru.getEnvVar("baseUrl"));"#).await;
    assert_eq!(out.vars.runtime["e"], json!("http://env"));
    assert_eq!(out.vars.environment["baseUrl"], json!("http://env"));
}

#[tokio::test]
async fn non_string_values_interpolate_like_runtime() {
    let values = json!({"n":3,"b":true,"z":null,"o":{"a":1}});
    let url = "/{{n}}/{{b}}/{{z}}/{{o}}";
    let from_row = ctx(values.clone(), json!({}), None)
        .vars
        .var_stack()
        .interpolate(url);
    let from_runtime = ctx(json!({}), values, None)
        .vars
        .var_stack()
        .interpolate(url);
    assert_eq!(from_row, from_runtime);
    assert_eq!(from_row, r#"/3/true/null/{"a":1}"#);
}

#[tokio::test]
async fn iteration_data_api() {
    let c = ctx(
        json!({"user":"alice","n":2}),
        json!({}),
        Some(IterationInfo { index: 1, total: 3 }),
    );
    let out = run(
        &c,
        r#"
        const d = bru.runner.iterationData;
        bru.setVar("all", d.get());
        bru.setVar("user", d.get("user"));
        d.set("k", "v");
        bru.setVar("hasK", d.has("k"));
        bru.setVar("hasNope", d.has("nope"));
        d.unset("user");
        bru.setVar("str", d.stringify());
        bru.setVar("idx", bru.runner.iterationIndex);
        bru.setVar("total", bru.runner.totalIterations);
        "#,
    )
    .await;
    let rt = &out.vars.runtime;
    assert_eq!(rt["all"], json!({"user":"alice","n":2}));
    assert_eq!(rt["user"], json!("alice"));
    assert_eq!(rt["hasK"], json!(true));
    assert_eq!(rt["hasNope"], json!(false));
    assert_eq!(rt["str"], json!(r#"{"n":2,"k":"v"}"#));
    assert_eq!(rt["idx"], json!(1));
    assert_eq!(rt["total"], json!(3));
    // Changes go back to the runner.
    assert_eq!(out.vars.iteration, map(json!({"n":2,"k":"v"})));
    // ...and a pre-request `set` changes the request's own interpolation.
    assert_eq!(out.vars.var_stack().interpolate("{{k}}"), "v");
}

#[tokio::test]
async fn unset_only_removes_truthy_values_like_bruno() {
    let c = ctx(json!({"zero":0,"empty":"","x":"y"}), json!({}), None);
    let out = run(
        &c,
        r#"const d = bru.runner.iterationData; d.unset("zero"); d.unset("empty"); d.unset("x");"#,
    )
    .await;
    assert_eq!(out.vars.iteration, map(json!({"zero":0,"empty":""})));
}

#[tokio::test]
async fn single_send_has_no_iteration() {
    let c = ctx(json!({}), json!({}), None);
    let out = run(
        &c,
        r#"
        bru.setVar("idx", String(bru.runner.iterationIndex));
        bru.setVar("total", String(bru.runner.totalIterations));
        bru.setVar("data", bru.runner.iterationData.get());
        "#,
    )
    .await;
    assert_eq!(out.vars.runtime["idx"], json!("undefined"));
    assert_eq!(out.vars.runtime["total"], json!("undefined"));
    assert_eq!(out.vars.runtime["data"], json!({}));
}

#[tokio::test]
async fn assert_rhs_sees_the_row() {
    let mut c = ctx(json!({"x":"row"}), json!({"x":"rt"}), None);
    c.res = Some(ScriptResponse {
        status: 200,
        body: r#"{"v":"row"}"#.into(),
        ..ScriptResponse::default()
    });
    let out = run_asserts(
        &engine(),
        &c,
        &[("res.body.v".to_owned(), "eq {{x}}".to_owned())],
    )
    .await;
    assert_eq!(out.asserts.len(), 1);
    assert!(out.asserts[0].passed, "{:?}", out.asserts);
}
