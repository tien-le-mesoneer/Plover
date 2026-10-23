//! `bru`, `req` and `res` as scripts see them (bruno-js/src/bru.js, bruno-request.js,
//! bruno-response.js).

use brs_core::script::{
    NextRequest, ScriptBody, ScriptContext, ScriptEngine, ScriptError, ScriptInfo, ScriptLimits,
    ScriptRequest, ScriptResponse, Segment, run_script,
};
use serde_json::json;

fn engine() -> ScriptEngine {
    ScriptEngine::new(ScriptLimits::default())
}

fn context() -> ScriptContext {
    let mut ctx = ScriptContext {
        info: ScriptInfo {
            env_name: Some("local".into()),
            collection_name: "demo".into(),
            collection_path: "/tmp/demo".into(),
            request_name: "get user".into(),
            ..ScriptInfo::default()
        },
        req: ScriptRequest {
            url: "{{host}}/users/1".into(),
            method: "POST".into(),
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("Authorization".into(), "Bearer {{token}}".into()),
            ],
            body: ScriptBody::Text {
                text: r#"{"name":"Ana","tags":["a"]}"#.into(),
            },
            timeout_ms: None,
            max_redirects: None,
        },
        ..ScriptContext::default()
    };
    ctx.vars
        .environment
        .insert("host".into(), json!("http://h"));
    ctx.vars
        .collection
        .insert("c".into(), json!("coll-{{host}}"));
    ctx.vars.folder.insert("f".into(), json!("folder"));
    ctx.vars.request.insert("r".into(), json!("req"));
    ctx
}

fn response() -> ScriptResponse {
    ScriptResponse {
        status: 201,
        status_text: "Created".into(),
        headers: vec![
            (
                "Content-Type".into(),
                "application/json; charset=utf-8".into(),
            ),
            ("set-cookie".into(), "a=1".into()),
            ("Set-Cookie".into(), "b=2".into()),
            ("x-multi".into(), "1".into()),
            ("X-Multi".into(), "2".into()),
        ],
        body: "\u{feff}{\"token\":\"t-1\",\"items\":[{\"id\":7}]}".into(),
        response_time_ms: 12,
        url: "http://h/users/1".into(),
        size: 40,
    }
}

async fn ok(ctx: &ScriptContext, src: &str) -> brs_core::script::PhaseOutput {
    let out = run_script(&engine(), ctx, vec![Segment::new("request", src)]).await;
    assert!(
        out.error.is_none(),
        "{:?}\nconsole: {:?}",
        out.error,
        out.console
    );
    out
}

#[tokio::test]
async fn runtime_vars_round_trip_any_json() {
    let out = ok(
        &context(),
        r#"
        bru.setVar('token', 'abc');
        bru.setVar('obj', { a: [1, 2] });
        bru.setVar('n', 3);
        if (bru.getVar('obj').a[1] !== 2) throw new Error('obj');
        if (!bru.hasVar('n') || bru.hasVar('nope')) throw new Error('has');
        bru.deleteVar('n');
        console.log(JSON.stringify(bru.getAllVars()));
        "#,
    )
    .await;
    assert_eq!(out.vars.runtime["token"], json!("abc"));
    assert_eq!(out.vars.runtime["obj"], json!({ "a": [1, 2] }));
    assert!(!out.vars.runtime.contains_key("n"));
    assert_eq!(out.console[0].text, r#"{"token":"abc","obj":{"a":[1,2]}}"#);
}

#[tokio::test]
async fn invalid_variable_name_throws_brunos_message() {
    let out = run_script(
        &engine(),
        &context(),
        vec![Segment::new("request", "bru.setVar('bad name', 1);")],
    )
    .await;
    match out.error {
        Some(ScriptError::Thrown { message, .. }) => assert_eq!(
            message,
            r#"Variable name: "bad name" contains invalid characters! Names must only contain alpha-numeric characters, "-", "_", ".""#
        ),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn env_and_scope_vars_with_interpolation() {
    let out = ok(
        &context(),
        r#"
        if (bru.getEnvName() !== 'local') throw new Error('env name');
        if (bru.getCollectionName() !== 'demo') throw new Error('collection name');
        if (bru.cwd() !== '/tmp/demo') throw new Error('cwd');
        bru.setEnvVar('host', 'http://other');
        if (bru.getEnvVar('host') !== 'http://other') throw new Error('env');
        if (!bru.hasEnvVar('host')) throw new Error('hasEnv');
        if (bru.getCollectionVar('c') !== 'coll-http://other') throw new Error('collection var ' + bru.getCollectionVar('c'));
        if (bru.getFolderVar('f') !== 'folder' || bru.getRequestVar('r') !== 'req') throw new Error('scope vars');
        bru.setVar('id', 9);
        console.log(bru.interpolate('{{host}}/x/{{id}}/{{missing}}'));
        console.log(JSON.stringify(bru.interpolate({ u: '{{host}}' })));
        "#,
    )
    .await;
    assert_eq!(out.vars.environment["host"], json!("http://other"));
    assert_eq!(out.console[0].text, "http://other/x/9/{{missing}}");
    assert_eq!(out.console[1].text, r#"{"u":"http://other"}"#);
}

#[tokio::test]
async fn req_reads_and_mutations() {
    let out = ok(
        &context(),
        r#"
        if (req.getUrl() !== '{{host}}/users/1' || req.url !== '{{host}}/users/1') throw new Error('url');
        if (req.getMethod() !== 'POST') throw new Error('method');
        if (req.getHeader('Authorization') !== 'Bearer {{token}}') throw new Error('header');
        if (req.getAuthMode() !== 'bearer') throw new Error('auth mode ' + req.getAuthMode());
        if (req.getName() !== 'get user') throw new Error('name');
        const body = req.getBody();
        if (body.name !== 'Ana') throw new Error('json body');
        if (req.getBody({ raw: true }) !== '{"name":"Ana","tags":["a"]}') throw new Error('raw body');
        req.setUrl('{{host}}/users/2');
        req.setMethod('PUT');
        req.setHeader('x-a', '1');
        req.deleteHeader('Authorization');
        req.setBody({ name: 'Bo' });
        req.setTimeout(1500);
        req.setMaxRedirects(2);
        "#,
    )
    .await;
    assert_eq!(out.req.url, "{{host}}/users/2");
    assert_eq!(out.req.method, "PUT");
    assert_eq!(
        out.req.headers,
        vec![
            ("content-type".to_owned(), "application/json".to_owned()),
            ("x-a".to_owned(), "1".to_owned())
        ]
    );
    assert_eq!(
        out.req.body,
        ScriptBody::Text {
            text: r#"{"name":"Bo"}"#.into()
        }
    );
    assert_eq!(out.req.timeout_ms, Some(1500));
    assert_eq!(out.req.max_redirects, Some(2));
}

#[tokio::test]
async fn res_properties_and_methods() {
    let mut ctx = context();
    ctx.res = Some(response());
    let out = ok(
        &ctx,
        r#"
        if (res.status !== 201 || res.getStatus() !== 201) throw new Error('status');
        if (res.statusText !== 'Created' || res.getStatusText() !== 'Created') throw new Error('statusText');
        if (res.body.items[0].id !== 7 || res.getBody().token !== 't-1') throw new Error('body');
        if (res.getHeader('Content-Type') !== 'application/json; charset=utf-8') throw new Error('header ci');
        if (res.headers['content-type'] !== 'application/json; charset=utf-8') throw new Error('headers lower');
        if (JSON.stringify(res.getHeader('set-cookie')) !== '["a=1","b=2"]') throw new Error('set-cookie ' + JSON.stringify(res.getHeader('set-cookie')));
        if (res.getHeader('x-multi') !== '1, 2') throw new Error('joined');
        if (res.getResponseTime() !== 12 || res.responseTime !== 12) throw new Error('time');
        if (res.getUrl() !== 'http://h/users/1') throw new Error('url');
        if (res('items[0].id') !== 7) throw new Error('query');
        if (res.getSize().body !== 40) throw new Error('size');
        res.setBody({ replaced: true });
        "#,
    )
    .await;
    assert_eq!(out.res_body, Some(json!({ "replaced": true })));
}

#[tokio::test]
async fn non_json_body_is_a_string() {
    let mut ctx = context();
    let mut r = response();
    r.body = "plain text".into();
    ctx.res = Some(r);
    let out = ok(&ctx, "console.log(typeof res.body, res.body);").await;
    assert_eq!(out.console[0].text, "string plain text");
}

#[tokio::test]
async fn flow_control() {
    let out = ok(
        &context(),
        "bru.runner.skipRequest(); bru.runner.stopExecution();",
    )
    .await;
    assert!(out.flow.skip_request && out.flow.stop_execution);
    assert_eq!(out.flow.next_request, None);
    let out = ok(&context(), "bru.setNextRequest('login');").await;
    assert_eq!(
        out.flow.next_request,
        Some(NextRequest::Name("login".into()))
    );
    let out = ok(&context(), "bru.runner.setNextRequest(null);").await;
    assert_eq!(out.flow.next_request, Some(NextRequest::Stop));
}

#[tokio::test]
async fn sleep_is_async() {
    let out = ok(
        &context(),
        "const t = Date.now(); await bru.sleep(30); console.log(Date.now() - t >= 25);",
    )
    .await;
    assert_eq!(out.console[0].text, "true");
}

#[tokio::test]
async fn unsupported_apis_say_so() {
    for (src, api) in [
        ("await bru.sendRequest({})", "bru.sendRequest"),
        ("await bru.runRequest('x')", "bru.runRequest"),
        ("bru.getGlobalEnvVar('x')", "bru.getGlobalEnvVar"),
        ("bru.cookies.jar()", "bru.cookies"),
    ] {
        let out = run_script(&engine(), &context(), vec![Segment::new("request", src)]).await;
        match out.error {
            Some(ScriptError::Thrown { message, .. }) => {
                assert_eq!(
                    message,
                    format!("{api} is not supported by bru-rs yet"),
                    "{src}"
                )
            }
            other => panic!("{src}: {other:?}"),
        }
    }
}

#[tokio::test]
async fn state_survives_a_thrown_error() {
    let out = run_script(
        &engine(),
        &context(),
        vec![Segment::new(
            "request",
            "bru.setVar('before', 1); throw new Error('x');",
        )],
    )
    .await;
    assert!(out.error.is_some());
    assert_eq!(out.vars.runtime["before"], json!(1));
}

#[tokio::test]
async fn interpolate_deep_chain_terminates() {
    let out = ok(
        &context(),
        "const N = 200000; for (let i = 0; i < N; i++) bru.setVar('a' + i, '{{a' + (i + 1) + '}}'); bru.setVar('a' + N, 'x'); console.log(bru.interpolate('{{a0}}'));",
    )
    .await;
    // Past the depth cap the placeholder stays literal instead of overflowing a stack.
    assert_eq!(out.console[0].text, "{{a1000}}");
}

#[tokio::test]
async fn interpolate_output_is_capped() {
    let out = run_script(
        &engine(),
        &context(),
        vec![Segment::new(
            "request",
            "for (let i = 0; i < 29; i++) bru.setVar('a' + i, '{{a' + (i + 1) + '}}{{a' + (i + 1) + '}}'); bru.setVar('a29', 'x'); bru.interpolate('{{a0}}');",
        )],
    )
    .await;
    match out.error {
        Some(ScriptError::Thrown { message, .. }) => assert!(
            message.starts_with("interpolating variables produced more than"),
            "{message}"
        ),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn interpolate_walks_object_variables() {
    let out = ok(
        &context(),
        "bru.setVar('user', { id: 42, tags: ['a', { b: 'x' }] }); console.log(bru.interpolate('{{user.id}}/{{user.tags[1].b}}'));",
    )
    .await;
    assert_eq!(out.console[0].text, "42/x");
}

async fn run_src(limits: ScriptLimits, src: &str) -> brs_core::script::PhaseOutput {
    run_script(
        &ScriptEngine::new(limits),
        &context(),
        vec![Segment::new("request", src)],
    )
    .await
}

fn result_error(out: &brs_core::script::PhaseOutput) -> String {
    match &out.error {
        Some(ScriptError::ResultUnavailable(message)) => message.clone(),
        other => panic!("expected ResultUnavailable, got {other:?}"),
    }
}

#[tokio::test]
async fn lone_surrogate_in_console_is_replaced_not_dropped() {
    let out = run_src(
        ScriptLimits::default(),
        "test('must fail', () => { expect(1).to.equal(2); }); console.log('a\\ud800b');",
    )
    .await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.console[0].text, "a\u{fffd}b");
    assert_eq!(out.tests.len(), 1);
    assert!(!out.tests[0].passed);
}

#[tokio::test]
async fn unserializable_vars_fail_closed_and_keep_other_results() {
    for (src, needle) in [
        ("bru.setVar('n', 1n);", "BigInt"),
        (
            "const o = {}; o.self = o; bru.setVar('cyc', o);",
            "circular",
        ),
        (
            "let v = 1; for (let i = 0; i < 200; i++) v = [v]; bru.setVar('deep', v);",
            "nested",
        ),
    ] {
        let out = run_src(
            ScriptLimits::default(),
            &format!("{src} test('still reported', () => {{ throw new Error('x'); }}); console.log('kept');"),
        )
        .await;
        let message = result_error(&out);
        assert!(
            message.contains("variables")
                && message.to_lowercase().contains(&needle.to_lowercase()),
            "{src}: {message}"
        );
        assert_eq!(out.tests.len(), 1, "{src}");
        assert_eq!(out.console[0].text, "kept", "{src}");
    }
}

#[tokio::test]
async fn out_of_memory_while_reporting_fails_closed() {
    let out = run_src(
        ScriptLimits {
            memory_bytes: 48 * 1024 * 1024,
            ..ScriptLimits::default()
        },
        "bru.setVar('big', 'x'.repeat(30e6)); test('t', () => {});",
    )
    .await;
    assert!(
        matches!(
            out.error,
            Some(ScriptError::ResultUnavailable(_)) | Some(ScriptError::OutOfMemory)
        ),
        "{:?}",
        out.error
    );
}
