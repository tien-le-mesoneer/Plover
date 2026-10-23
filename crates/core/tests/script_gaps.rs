//! Script gaps A–D: process.env/.env, extra safe-mode modules, sendRequest/axios/cookies,
//! persisted variable changes (docs/superpowers/plans/2026-10-23-script-gaps.md).

use std::fs;
use std::path::Path;

use brs_core::collection::Collection;
use brs_core::executor::{ExecOptions, Session};
use brs_core::runner::{RunEvent, RunOptions, RunReport, run, run_with};
use brs_core::script::ScriptLimits;
use brs_core::vars::ValueMap;
use serde_json::json;
use std::time::{Duration, Instant};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn setup(server: &MockServer, flow: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let scripts = flow
        .map(|f| format!(r#","scripts":{{"flow":"{f}"}}"#))
        .unwrap_or_default();
    write(
        dir.path(),
        "bruno.json",
        &format!(r#"{{"version":"1","name":"chain","type":"collection"{scripts}}}"#),
    );
    write(
        dir.path(),
        "environments/local.bru",
        &format!("vars {{\n  host: {}\n}}\n", server.uri()),
    );
    dir
}

/// A GET request file; `blocks` are appended verbatim.
fn request(name: &str, seq: u32, url_path: &str, blocks: &str) -> String {
    format!(
        "meta {{\n  name: {name}\n  type: http\n  seq: {seq}\n}}\n\nget {{\n  url: {{{{host}}}}{url_path}\n  body: none\n  auth: none\n}}\n{blocks}"
    )
}

fn text_block(name: &str, body: &str) -> String {
    let indented: Vec<String> = body.lines().map(|l| format!("  {l}")).collect();
    format!("\n{name} {{\n{}\n}}\n", indented.join("\n"))
}

async fn run_dir(root: &Path) -> (RunReport, Vec<RunEvent>) {
    let c = Collection::load(root).unwrap();
    let mut events = Vec::new();
    let opts = RunOptions {
        env: Some("local".into()),
        ..RunOptions::default()
    };
    let report = run(&c, root, &opts, &mut |e| events.push(e.clone()))
        .await
        .unwrap();
    (report, events)
}

fn failures(events: &[RunEvent]) -> Vec<String> {
    let mut out = Vec::new();
    for e in events {
        let outcome = match e {
            RunEvent::Passed { outcome, .. } => outcome,
            RunEvent::Failed { error, outcome, .. } => {
                out.push(error.clone());
                outcome
            }
            _ => continue,
        };
        for t in outcome.tests.iter().filter(|t| !t.passed) {
            out.push(format!("{}: {:?}", t.name, t.error));
        }
        if let Some(e) = &outcome.script_error {
            out.push(e.error.to_string());
        }
    }
    out
}

#[tokio::test]
async fn process_env_and_dotenv() {
    let server = MockServer::start().await;
    Mock::given(path("/from-dotenv"))
        .and(header("authorization", "Bearer dot"))
        .and(header("x-missing", "{{process.env.BRS_SURELY_MISSING}}"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = setup(&server, None);
    write(dir.path(), ".env", "# comment\nAPI_PATH=/from-dotenv\nTOKEN=\"dot\"\n");
    write(
        dir.path(),
        "environments/local.bru",
        &format!("vars {{\n  host: {}\n  auth: Bearer {{{{process.env.TOKEN}}}}\n}}\n", server.uri()),
    );
    let tests = text_block(
        "tests",
        "test('process env', () => {\n  expect(bru.getProcessEnv('TOKEN')).to.equal('dot');\n  expect(bru.getProcessEnv('BRS_SURELY_MISSING')).to.equal(undefined);\n  expect(typeof bru.getProcessEnv('PATH')).to.equal('string');\n  expect(bru.interpolate('{{process.env.API_PATH}}')).to.equal('/from-dotenv');\n  expect(bru.getEnvVar('auth')).to.equal('Bearer dot');\n});",
    );
    let headers = "\nheaders {\n  authorization: {{auth}}\n  x-missing: {{process.env.BRS_SURELY_MISSING}}\n}\n";
    write(
        dir.path(),
        "r.bru",
        &request("r", 1, "{{process.env.API_PATH}}", &format!("{headers}{tests}")),
    );
    let (report, events) = run_dir(dir.path()).await;
    assert_eq!(failures(&events), Vec::<String>::new());
    assert_eq!((report.passed, report.tests_passed), (1, 1));
}

const RSA_KEY: &str = include_str!("fixtures/jwt/test-only-rsa.key");
const RSA_PUB: &str = include_str!("fixtures/jwt/test-only-rsa.pub");
const OTHER_PUB: &str = include_str!("fixtures/jwt/test-only-other.pub");

/// One request whose post-response script is `script`; every `test()` in it must pass.
async fn run_script_ok(server: &MockServer, url_path: &str, script: &str, vars: &str) -> Vec<RunEvent> {
    let dir = setup(server, None);
    write(
        dir.path(),
        "environments/local.bru",
        &format!("vars {{\n  host: {}\n{vars}}}\n", server.uri()),
    );
    write(
        dir.path(),
        "r.bru",
        &request("r", 1, url_path, &text_block("script:post-response", script)),
    );
    let (report, events) = run_dir(dir.path()).await;
    assert_eq!(failures(&events), Vec::<String>::new());
    assert_eq!(report.tests_failed, 0);
    assert!(report.tests_passed > 0, "no tests ran");
    events
}

fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

#[tokio::test]
async fn safe_mode_uuid_nanoid_path() {
    let server = MockServer::start().await;
    Mock::given(path("/x")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
    // Expected values from Bruno CLI 4.3.0 --sandbox safe (uuid 10.0.0).
    let script = r#"
const uuid = require('uuid');
test('uuid', () => {
  expect(uuid.validate(uuid.v4())).to.equal(true);
  expect(uuid.version(uuid.v4())).to.equal(4);
  expect(uuid.v4()).to.not.equal(uuid.v4());
  expect(uuid.v5('hello', '6ba7b810-9dad-11d1-80b4-00c04fd430c8')).to.equal('9342d47a-1bab-5709-9869-c840b2eac501');
  expect(uuid.v3('hello', '6ba7b810-9dad-11d1-80b4-00c04fd430c8')).to.equal('0bacede4-4014-3f9d-b720-173f68a1c933');
  expect(uuid.validate(uuid.v7())).to.equal(true);
  expect(Object.keys(uuid).sort()).to.eql(['parse','stringify','v1','v1ToV6','v3','v4','v5','v6','v6ToV1','v7','validate','version']);
  expect(globalThis.uuid.v4).to.be.a('function');
});
test('nanoid', () => {
  const { nanoid } = require('nanoid');
  expect(nanoid()).to.match(/^[A-Za-z0-9_-]{21}$/);
  expect(nanoid()).to.not.equal(nanoid());
  expect(Object.keys(require('nanoid'))).to.eql(['nanoid']);
});
test('path', () => {
  const path = require('path');
  expect(path.resolve('/a/b', '../c', './d')).to.equal('/a/c/d');
  expect(path.resolve('lib/x.js')).to.equal(bru.cwd() + '/lib/x.js');
  expect(Object.keys(path)).to.eql(['resolve']);
});
test('other modules still forbidden', () => {
  expect(() => require('fs')).to.throw("Cannot find module 'fs'");
});
"#;
    run_script_ok(&server, "/x", script, "").await;
}

#[tokio::test]
async fn safe_mode_jsonwebtoken_matches_node() {
    let server = MockServer::start().await;
    Mock::given(path("/x")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
    // Tokens produced by Bruno CLI 4.3.0 --sandbox safe and node jsonwebtoken 9.0.3.
    let script = format!(
        r#"
const jwt = require('jsonwebtoken');
const RSA_KEY = {rsa_key};
const RSA_PUB = {rsa_pub};
const OTHER_PUB = {other_pub};
test('HS256/HS512 sign matches node', () => {{
  expect(jwt.sign({{ a: 1, iat: 1700000000 }}, 'secret')).to.equal('eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJhIjoxLCJpYXQiOjE3MDAwMDAwMDB9.Kyhsshc_ttIN1uv6O8ja2QzegqAnunFUREhg_a5tYoI');
  expect(jwt.sign({{ a: 1, iat: 1700000000 }}, 'secret', {{ expiresIn: '1h', audience: 'aud', issuer: 'me', keyid: 'k1', algorithm: 'HS512' }})).to.equal('eyJhbGciOiJIUzUxMiIsInR5cCI6IkpXVCIsImtpZCI6ImsxIn0.eyJhIjoxLCJpYXQiOjE3MDAwMDAwMDAsImV4cCI6MTcwMDAwMzYwMCwiYXVkIjoiYXVkIiwiaXNzIjoibWUifQ.MLXNjhq1ix58O10AsCW-tUXJo70drvzj4W0b5BVMgTsqKzRh91S6bRWJO_d4rwJXvb94iikK7p3Ot-OEfOkuTA');
  expect(jwt.sign('plain', 'secret')).to.equal('eyJhbGciOiJIUzI1NiJ9.cGxhaW4.e_JCoMzTbTiyewyi7By3NLvZLOL5-7_A9SG1JKmV1Mc');
  expect(jwt.sign({{ 'ü': 'ünï', iat: 1700000000 }}, 'sécret')).to.equal('eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyLDvCI6IsO8bsOvIiwiaWF0IjoxNzAwMDAwMDAwfQ.3BFIVL-p_OEwjGOVG7smXd517dF71aDKm1Ii659204U');
  expect(jwt.sign({{ a: 1, iat: 1700000000 }}, 'secret', {{ notBefore: 60, jwtid: 'j1', subject: 's', header: {{ x: 1 }} }})).to.equal('eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCIsIngiOjF9.eyJhIjoxLCJpYXQiOjE3MDAwMDAwMDAsIm5iZiI6MTcwMDAwMDA2MCwic3ViIjoicyIsImp0aSI6ImoxIn0.boJERsx5vKAoBE2LF9EU1SfEcZq4ydCqTzoZB-GKxWk');
}});
test('RS256 sign matches node and verifies', () => {{
  const token = jwt.sign({{ sub: 'u1', iat: 1700000000 }}, RSA_KEY, {{ algorithm: 'RS256' }});
  expect(token).to.equal('eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ1MSIsImlhdCI6MTcwMDAwMDAwMH0.GLn8GeYgi1apnvQ5lp_yaO-vKduQSBFEAVOYxLDtPf2NQHVeJQpRGBkeZtNdMsDw3PhZG3_5FRhK_-SI-uaLaCh9VZIjehFM4XhHXb00NS-2_i0r1JjBGhg0nGRP2oPMyx9MMrymAI8gZ3NygeCPlQIypBXGLjI-gps8bw24DCgMQULvOb6cSCZrTR7CiP8A394iWx8UMnAnswRZn5DbUdpFraVQdMQHlI0fEetUNxtHVQG8P5Ghowq0OZM8mAJgSYnrNqJwQ5PV06Csq4WNMC3zDnCgz8VKwKvlsPRFGq-0DBmuAcGz0ifnyD2FYEcQWXlhbDOBe8w8A0XLftJxJg');
  expect(jwt.verify(token, RSA_PUB).sub).to.equal('u1');
  expect(() => jwt.verify(token, OTHER_PUB)).to.throw('invalid signature');
}});
test('verify and decode', () => {{
  const ok = jwt.sign({{ a: 1, iat: 1700000000 }}, 'secret');
  expect(jwt.verify(ok, 'secret')).to.eql({{ a: 1, iat: 1700000000 }});
  expect(jwt.decode(ok, {{ complete: true }})).to.eql({{ header: {{ alg: 'HS256', typ: 'JWT' }}, payload: {{ a: 1, iat: 1700000000 }}, signature: 'Kyhsshc_ttIN1uv6O8ja2QzegqAnunFUREhg_a5tYoI' }});
  expect(jwt.decode('nope')).to.equal(null);
  let e;
  try {{ jwt.verify(jwt.sign({{ a: 1, iat: 1700000000, exp: 1700000001 }}, 'secret'), 'secret'); }} catch (err) {{ e = err; }}
  expect([e.name, e.message]).to.eql(['Error', 'jwt expired']);
  expect(() => jwt.verify(jwt.sign({{ a: 1 }}, 'secret'), 'other')).to.throw('invalid signature');
  expect(() => jwt.verify(jwt.sign({{ a: 1, aud: 'x' }}, 'secret'), 'secret', {{ audience: 'y' }})).to.throw('jwt audience invalid. expected: y');
  expect(() => jwt.verify('a.b', 'secret')).to.throw('jwt malformed');
  expect(jwt.verify(jwt.sign({{ a: 1, exp: 1 }}, 'secret'), 'secret', {{ ignoreExpiration: true }}).a).to.equal(1);
}});
test('callbacks', async () => {{
  const token = await new Promise((resolve, reject) => jwt.sign({{ a: 1 }}, 'secret', (err, t) => (err ? reject(err) : resolve(t))));
  const decoded = await new Promise((resolve) => jwt.verify(token, 'secret', {{}}, (err, d) => resolve(err || d)));
  expect(decoded.a).to.equal(1);
  expect(globalThis.jwt.sign).to.be.a('function');
}});
"#,
        rsa_key = js_string(RSA_KEY),
        rsa_pub = js_string(RSA_PUB),
        other_pub = js_string(OTHER_PUB),
    );
    run_script_ok(&server, "/x", &script, "").await;
}

fn outcome(events: &[RunEvent]) -> brs_core::runner::RequestOutcome {
    events
        .iter()
        .find_map(|e| match e {
            RunEvent::Passed { outcome, .. } | RunEvent::Failed { outcome, .. } => Some(outcome.clone()),
            _ => None,
        })
        .expect("a finished request")
}

#[tokio::test]
async fn send_request_and_axios_shapes() {
    let server = MockServer::start().await;
    Mock::given(path("/x")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
    Mock::given(method("POST"))
        .and(path("/items"))
        .and(query_param("q", "a b"))
        .and(header("x-h", "v"))
        .and(header("content-type", "application/json"))
        .and(body_json(json!({ "a": 1 })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": 7 })))
        .mount(&server)
        .await;
    Mock::given(path("/404"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({ "error": "nf" })))
        .mount(&server)
        .await;
    Mock::given(path("/text"))
        .respond_with(ResponseTemplate::new(200).set_body_string("plain text"))
        .mount(&server)
        .await;
    // Shapes from Bruno CLI 4.3.0 --sandbox safe.
    let script = r#"
const host = bru.getEnvVar('host');
test('sendRequest resolves an axios-like response', async () => {
  const res = await bru.sendRequest({ url: host + '/items', method: 'post', params: { q: 'a b' }, data: { a: 1 }, headers: { 'X-H': 'v' } });
  expect(Object.keys(res).sort()).to.eql(['config', 'data', 'headers', 'request', 'responseTime', 'status', 'statusText']);
  expect([res.status, res.statusText, res.data.id, typeof res.headers]).to.eql([201, 'Created', 7, 'object']);
  expect(res.headers['content-type']).to.equal('application/json');
  const t = await bru.sendRequest(host + '/text');
  expect([t.data, typeof t.data]).to.eql(['plain text', 'string']);
});
test('sendRequest rejects 4xx without response', async () => {
  let e;
  try { await bru.sendRequest({ url: host + '/404' }); } catch (err) { e = err; }
  expect(Object.keys(e).sort()).to.eql(['code', 'config', 'message', 'name', 'stack', 'status']);
  expect([e.message, e.code, e.status, e.name, e.response]).to.eql(['Request failed with status code 404', 'ERR_BAD_REQUEST', 404, 'AxiosError', undefined]);
  let cbErr, cbRes = 'unset';
  await bru.sendRequest({ url: host + '/404' }, (err, res) => { cbErr = err; cbRes = res; });
  expect([cbErr.status, cbRes]).to.eql([404, null]);
  const ok = await bru.sendRequest({ url: host + '/text' }, (err, res) => { expect(err).to.equal(null); expect(res.status).to.equal(200); });
  expect(ok.status).to.equal(200);
});
test('axios', async () => {
  const axios = require('axios');
  const res = await axios.post(host + '/items?q=a%20b', { a: 1 }, { headers: { 'X-H': 'v' } });
  expect(Object.keys(res).sort()).to.eql(['data', 'headers', 'status']);
  expect([res.status, res.data.id]).to.eql([201, 7]);
  let e;
  try { await axios.get(host + '/404'); } catch (err) { e = err; }
  expect(Object.keys(e).sort()).to.eql(['code', 'config', 'isAxiosError', 'message', 'response']);
  expect([e.code, e.isAxiosError, e.response.status, e.response.data]).to.eql(['ERR_BAD_REQUEST', true, 404, { error: 'nf' }]);
  const kept = await axios({ url: host + '/404', validateStatus: () => true });
  expect(kept.status).to.equal(404);
  let refused;
  try { await axios.get('http://127.0.0.1:9/'); } catch (err) { refused = err; }
  expect(refused.code).to.equal('ECONNREFUSED');
  expect(globalThis.axios).to.equal(axios);
});
"#;
    let events = run_script_ok(&server, "/x", script, "").await;
    let scripted = outcome(&events).scripted_requests;
    // Async tests interleave, so compare without order.
    let mut summary: Vec<(String, String, Option<u16>)> = scripted
        .iter()
        .map(|r| (r.source.clone(), r.method.clone(), r.status))
        .collect();
    summary.sort();
    let mut expected: Vec<(String, String, Option<u16>)> = vec![
            ("sendRequest".into(), "POST".into(), Some(201)),
            ("sendRequest".into(), "GET".into(), Some(200)),
            ("sendRequest".into(), "GET".into(), Some(404)),
            ("sendRequest".into(), "GET".into(), Some(404)),
            ("sendRequest".into(), "GET".into(), Some(200)),
            ("axios".into(), "POST".into(), Some(201)),
            ("axios".into(), "GET".into(), Some(404)),
            ("axios".into(), "GET".into(), Some(404)),
            ("axios".into(), "GET".into(), None),
        ];
    expected.sort();
    assert_eq!(summary, expected);
    assert!(scripted.iter().any(|r| r.status.is_none() && r.error.is_some()));
}

/// Bruno's sendRequest has its own axios instance: the run's cookies are not sent
/// (Bruno CLI 4.3.0 probe: no `cookie` header after a Set-Cookie response).
#[tokio::test]
async fn send_request_does_not_share_the_cookie_jar() {
    let server = MockServer::start().await;
    Mock::given(path("/login"))
        .respond_with(ResponseTemplate::new(200).insert_header("set-cookie", "sid=abc; Path=/"))
        .mount(&server)
        .await;
    Mock::given(path("/echo")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
    let script = "test('sent', async () => { await bru.sendRequest(bru.getEnvVar('host') + '/echo'); });";
    run_script_ok(&server, "/login", script, "").await;
    let received = server.received_requests().await.unwrap();
    let echo = received.iter().find(|r| r.url.path() == "/echo").unwrap();
    assert!(!echo.headers.contains_key("cookie"), "{:?}", echo.headers);
}

#[tokio::test]
async fn cookies_api_uses_the_session_jar() {
    let server = MockServer::start().await;
    Mock::given(path("/login"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("set-cookie", "sid=abc; Path=/; HttpOnly")
                .append_header("set-cookie", "theme=dark; Path=/"),
        )
        .mount(&server)
        .await;
    Mock::given(path("/next")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
    let dir = setup(&server, None);
    // Shapes from Bruno CLI 4.3.0 --sandbox safe.
    let script = r#"
const host = bru.getEnvVar('host');
test('bru.cookies reads the jar for the request URL', () => {
  expect(bru.cookies.get('sid')).to.equal('abc');
  expect(bru.cookies.toObject()).to.eql({ sid: 'abc', theme: 'dark' });
  expect(bru.cookies.has('theme', 'dark')).to.equal(true);
  expect(bru.cookies.count()).to.equal(2);
  const sid = bru.cookies.one('sid');
  expect(sid).to.eql({ key: 'sid', value: 'abc', domain: '127.0.0.1', path: '/', secure: false, httpOnly: true, expires: 'Infinity' });
  expect(bru.cookies.toString()).to.equal('sid=abc; theme=dark');
});
test('jar methods', async () => {
  const jar = bru.cookies.jar();
  const c = await jar.getCookie(host, 'sid');
  expect([c.key, c.value, c.domain, c.path, c.httpOnly, c.hostOnly, c.secure]).to.eql(['sid', 'abc', '127.0.0.1', '/', true, true, undefined]);
  expect(await jar.getCookie(host, 'nope')).to.equal(null);
  await jar.setCookie(host, 'k2', 'v2');
  expect(await jar.hasCookie(host, 'k2')).to.equal(true);
  await jar.setCookie(host, { key: 'k3', value: 'v3', path: '/' });
  await jar.deleteCookie(host, 'theme');
  expect((await jar.getCookies(host)).map((x) => x.key).sort()).to.eql(['k2', 'k3', 'sid']);
  let viaCallback;
  await jar.getCookies(host, (err, list) => { viaCallback = list.length; });
  expect(viaCallback).to.equal(3);
});
"#;
    write(dir.path(), "a.bru", &request("a", 1, "/login", &text_block("script:post-response", script)));
    write(dir.path(), "b.bru", &request("b", 2, "/next", ""));
    let (report, events) = run_dir(dir.path()).await;
    assert_eq!(failures(&events), Vec::<String>::new());
    assert_eq!((report.passed, report.tests_passed), (2, 2));
    // The next request sends what the script left in the jar.
    let received = server.received_requests().await.unwrap();
    let next = received.iter().find(|r| r.url.path() == "/next").unwrap();
    let mut sent: Vec<String> = next
        .headers
        .get_all("cookie")
        .iter()
        .flat_map(|v| v.to_str().unwrap().split("; ").map(str::to_owned).collect::<Vec<_>>())
        .collect();
    sent.sort();
    assert_eq!(sent, vec!["k2=v2", "k3=v3", "sid=abc"]);
}

#[tokio::test]
async fn send_request_respects_script_timeout_and_body_cap() {
    let server = MockServer::start().await;
    Mock::given(path("/x")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
    Mock::given(path("/slow"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;
    Mock::given(path("/big"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(1000)))
        .mount(&server)
        .await;
    let dir = setup(&server, None);
    write(
        dir.path(),
        "a.bru",
        &request(
            "a",
            1,
            "/x",
            &text_block(
                "script:post-response",
                "const r = await bru.sendRequest(bru.getEnvVar('host') + '/big');\nbru.setVar('bigLen', r.data.length);",
            ),
        ),
    );
    write(
        dir.path(),
        "b.bru",
        &request(
            "b",
            2,
            "/x",
            &text_block("script:post-response", "await bru.sendRequest(bru.getEnvVar('host') + '/slow');"),
        ),
    );
    let c = Collection::load(dir.path()).unwrap();
    let opts = RunOptions {
        env: Some("local".into()),
        scripts: ScriptLimits {
            timeout: Duration::from_millis(500),
            ..ScriptLimits::default()
        },
        ..RunOptions::default()
    };
    // The server caps bodies at a size; sendRequest goes through the same session.
    let session = Session::new(ExecOptions {
        max_body_bytes: Some(10),
        ..ExecOptions::default()
    })
    .unwrap();
    let mut runtime = ValueMap::new();
    let mut events = Vec::new();
    let started = Instant::now();
    run_with(&c, dir.path(), &opts, &session, &mut runtime, &mut |e| events.push(e.clone()))
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    assert_eq!(runtime.get("bigLen"), Some(&json!(10)));
    let errors = failures(&events);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors.iter().any(|e| e.contains("timed out after 500 ms")), "{errors:?}");
}
