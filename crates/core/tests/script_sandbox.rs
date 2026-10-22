//! The script sandbox on its own: libraries, `require`, limits and error reporting.

use std::time::{Duration, Instant};

use brs_core::script::{
    EngineJob, LogLevel, Main, ScriptEngine, ScriptError, ScriptLimits, Segment,
};

fn engine() -> ScriptEngine {
    ScriptEngine::new(ScriptLimits::default())
}

async fn run(engine: &ScriptEngine, src: &str) -> brs_core::script::EngineOutput {
    engine
        .run(EngineJob {
            input: serde_json::json!({}),
            main: Main::Segments(vec![Segment::new("request", src)]),
        })
        .await
}

fn thrown_message(err: &Option<ScriptError>) -> String {
    match err {
        Some(ScriptError::Thrown { message, .. }) => message.clone(),
        other => panic!("expected a thrown error, got {other:?}"),
    }
}

#[tokio::test]
async fn expect_and_assert_available() {
    let e = engine();
    let ok = run(&e, "expect(1).to.equal(1); assert.equal(2, 2);").await;
    assert!(ok.error.is_none(), "{:?}", ok.error);
    let bad = run(&e, "expect(1).to.equal(2);").await;
    assert_eq!(thrown_message(&bad.error), "expected 1 to equal 2");
}

#[tokio::test]
async fn require_safe_set_works() {
    let e = engine();
    let out = run(
        &e,
        r#"
        if (require('crypto-js').MD5('a').toString() !== '0cc175b9c0f1b6a831c399e269772661') throw new Error('md5');
        if (typeof require('moment') !== 'function') throw new Error('moment');
        if (Buffer.from('hi').toString('base64') !== 'aGk=') throw new Error('buffer');
        if (btoa('hi') !== 'aGk=' || atob('aGk=') !== 'hi') throw new Error('btoa');
        const Ajv = require('ajv');
        const v = new Ajv().compile({ type: 'object', required: ['a'] });
        if (!v({ a: 1 }) || v({})) throw new Error('ajv');
        if (require('chai').expect !== expect) throw new Error('chai');
        if (require('tv4') !== tv4 || typeof Ajv !== 'function' || typeof addFormats !== 'function') throw new Error('globals');
        "#,
    )
    .await;
    assert!(out.error.is_none(), "{:?}", out.error);
}

#[tokio::test]
async fn require_outside_safe_set_fails() {
    let e = engine();
    for module in ["fs", "axios"] {
        let out = run(&e, &format!("require('{module}');")).await;
        match out.error {
            Some(ScriptError::ForbiddenModule {
                ref module_name,
                ref message,
            }) => {
                assert_eq!(module_name, module);
                assert!(
                    message.contains(&format!("Cannot find module '{module}'")),
                    "{message}"
                );
                assert!(message.contains("chai, buffer, moment"), "{message}");
            }
            other => panic!("{module}: expected ForbiddenModule, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn no_ambient_globals() {
    let e = engine();
    let out = run(
        &e,
        "console.log(typeof process, typeof fetch, typeof XMLHttpRequest, typeof std, typeof os);",
    )
    .await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(
        out.console[0].text,
        "undefined undefined undefined undefined undefined"
    );
}

#[tokio::test]
async fn timers_and_top_level_await() {
    let e = engine();
    let out = run(
        &e,
        r#"
        const order = [];
        setTimeout(() => order.push('b'), 20);
        const id = setTimeout(() => order.push('never'), 10);
        clearTimeout(id);
        order.push('a');
        await new Promise(r => setTimeout(r, 40));
        console.log(order.join(','));
        "#,
    )
    .await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.console[0].text, "a,b");
}

#[tokio::test]
async fn infinite_loop_times_out() {
    let e = ScriptEngine::new(ScriptLimits {
        timeout: Duration::from_millis(200),
        ..ScriptLimits::default()
    });
    let start = Instant::now();
    let out = run(&e, "while (true) {}").await;
    assert!(
        matches!(out.error, Some(ScriptError::Timeout(_))),
        "{:?}",
        out.error
    );
    assert!(
        start.elapsed() < Duration::from_millis(1200),
        "{:?}",
        start.elapsed()
    );
    // The engine keeps working afterwards.
    let ok = run(&e, "console.log('alive')").await;
    assert!(ok.error.is_none(), "{:?}", ok.error);
}

#[tokio::test]
async fn timer_past_deadline_times_out() {
    let e = ScriptEngine::new(ScriptLimits {
        timeout: Duration::from_millis(200),
        ..ScriptLimits::default()
    });
    let start = Instant::now();
    let out = run(&e, "await new Promise(r => setTimeout(r, 60000));").await;
    assert!(
        matches!(out.error, Some(ScriptError::Timeout(_))),
        "{:?}",
        out.error
    );
    assert!(start.elapsed() < Duration::from_millis(1200));
}

#[tokio::test]
async fn never_settling_promise_is_reported() {
    let e = engine();
    let start = Instant::now();
    let out = run(&e, "await new Promise(() => {});").await;
    assert!(
        matches!(out.error, Some(ScriptError::Unsettled)),
        "{:?}",
        out.error
    );
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn memory_bomb_is_contained() {
    let e = ScriptEngine::new(ScriptLimits {
        memory_bytes: 32 * 1024 * 1024,
        ..ScriptLimits::default()
    });
    let out = run(&e, "const a = []; for (;;) a.push(new Array(1e6).fill(1));").await;
    assert!(
        matches!(out.error, Some(ScriptError::OutOfMemory)),
        "{:?}",
        out.error
    );
    let ok = run(&e, "console.log('alive')").await;
    assert!(ok.error.is_none(), "{:?}", ok.error);
    assert_eq!(ok.console[0].text, "alive");
}

#[tokio::test]
async fn throw_reports_scope_and_line() {
    let e = engine();
    let out = e
        .run(EngineJob {
            input: serde_json::json!({}),
            main: Main::Segments(vec![
                Segment::new("collection", "const x = 1;\nconsole.log('c');"),
                Segment::new("folder users", "\n\nthrow new Error('boom');"),
                Segment::new("request", "console.log('never');"),
            ]),
        })
        .await;
    match out.error {
        Some(ScriptError::Thrown {
            ref message,
            ref scope,
            line,
            ..
        }) => {
            assert_eq!(message, "boom");
            assert_eq!(scope.as_deref(), Some("folder users"));
            assert_eq!(line, Some(3));
        }
        other => panic!("expected Thrown, got {other:?}"),
    }
    assert_eq!(out.console.len(), 1);
}

#[tokio::test]
async fn segments_have_separate_scopes() {
    let e = engine();
    let out = e
        .run(EngineJob {
            input: serde_json::json!({}),
            main: Main::Segments(vec![
                Segment::new(
                    "collection",
                    "const x = 1; if (true) return; console.log('skipped');",
                ),
                Segment::new("request", "const x = 2; console.log(x);"),
            ]),
        })
        .await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.console.len(), 1);
    assert_eq!(out.console[0].text, "2");
}

#[tokio::test]
async fn console_levels_and_formatting() {
    let e = engine();
    let out = run(
        &e,
        "console.log('a', 1, {b: [1, 'x']}, null, undefined); console.warn('w'); console.error(new Error('e'));",
    )
    .await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.console[0].level, LogLevel::Log);
    assert_eq!(out.console[0].text, r#"a 1 {"b":[1,"x"]} null undefined"#);
    assert_eq!(out.console[1].level, LogLevel::Warn);
    assert_eq!(out.console[2].level, LogLevel::Error);
    assert!(
        out.console[2].text.starts_with("Error: e"),
        "{}",
        out.console[2].text
    );
}

#[tokio::test]
async fn fresh_context_per_run() {
    let e = engine();
    run(&e, "globalThis.leak = 1;").await;
    let out = run(&e, "console.log(typeof leak);").await;
    assert_eq!(out.console[0].text, "undefined");
}

#[tokio::test]
async fn per_run_overhead_is_reported() {
    let e = engine();
    run(&e, "").await; // warm the worker
    let n = 20;
    let start = Instant::now();
    for _ in 0..n {
        run(&e, "expect(1).to.equal(1);").await;
    }
    eprintln!("per-script overhead: {:?}", start.elapsed() / n);
}
