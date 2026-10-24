//! Test output, JUnit and JSON reports from `plv run` (Bruno CLI reporters:
//! bruno-cli/src/reporters/junit.js, commands/run.js `--reporter-json`).

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use quick_xml::Reader;
use quick_xml::events::Event;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn request(name: &str, seq: u32, url: &str, blocks: &str) -> String {
    format!(
        "meta {{\n  name: {name}\n  type: http\n  seq: {seq}\n}}\n\nget {{\n  url: {url}\n  body: none\n  auth: none\n}}\n{blocks}"
    )
}

/// a: two tests (one fails) and one passing assert; b: transport error; c: skipped.
async fn collection(server: &MockServer) -> tempfile::TempDir {
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"rep","type":"collection"}"#,
    );
    write(
        root,
        "a.bru",
        &request(
            "a",
            1,
            &format!("{}/a", server.uri()),
            "\nassert {\n  res.status: eq 200\n}\n\ntests {\n  test('ok', () => expect(res.status).to.equal(200));\n  test('a<b & \"c\"', () => expect(1).to.equal(2));\n  console.log('from a');\n}\n",
        ),
    );
    write(root, "b.bru", &request("b", 2, "http://127.0.0.1:9/b", ""));
    write(
        root,
        "c.bru",
        &request(
            "c",
            3,
            &format!("{}/c", server.uri()),
            "\nscript:pre-request {\n  bru.runner.skipRequest();\n}\n",
        ),
    );
    dir
}

#[derive(Debug, Default, Clone)]
struct Suite {
    attrs: Vec<(String, String)>,
    cases: Vec<Vec<(String, String)>>,
    failures: Vec<String>,
    errors: Vec<String>,
    skipped: Vec<String>,
}

impl Suite {
    fn attr(&self, name: &str) -> &str {
        &self.attrs.iter().find(|(k, _)| k == name).unwrap().1
    }
}

fn attrs(e: &quick_xml::events::BytesStart<'_>) -> Vec<(String, String)> {
    e.attributes()
        .map(|a| {
            let a = a.unwrap();
            (
                a.key.as_ref().to_owned(),
                a.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                    .unwrap()
                    .into_owned(),
            )
        })
        .collect()
}

fn parse_junit(xml: &str) -> Vec<Suite> {
    let mut reader = Reader::from_str(xml);
    let mut suites: Vec<Suite> = Vec::new();
    let mut root_seen = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.name().as_ref().to_owned();
                let a = attrs(&e);
                let message = || {
                    a.iter()
                        .find(|(k, _)| k == "message")
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default()
                };
                match name.as_str() {
                    "testsuites" => root_seen = true,
                    "testsuite" => suites.push(Suite {
                        attrs: a,
                        ..Suite::default()
                    }),
                    "testcase" => suites.last_mut().unwrap().cases.push(a),
                    "failure" => suites.last_mut().unwrap().failures.push(message()),
                    "error" => suites.last_mut().unwrap().errors.push(message()),
                    "skipped" => suites.last_mut().unwrap().skipped.push(message()),
                    other => panic!("unexpected element {other}"),
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    assert!(root_seen);
    suites
}

#[tokio::test]
async fn junit_matches_bruno_layout() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let out = dir.path().join("out");
    fs::create_dir(&out).unwrap();
    let junit = out.join("results.xml");
    Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(dir.path())
        .arg("--reporter-junit")
        .arg(&junit)
        .assert()
        .code(1);
    let xml = fs::read_to_string(&junit).unwrap();
    assert!(
        xml.starts_with("<?xml version=\"1.0\"?>\n<testsuites>"),
        "{xml}"
    );
    let suites = parse_junit(&xml);
    assert_eq!(suites.len(), 3);

    let a = &suites[0];
    assert_eq!(a.attr("name"), "a");
    assert_eq!(a.attr("file"), "a.bru");
    assert_eq!(
        (
            a.attr("tests"),
            a.attr("failures"),
            a.attr("errors"),
            a.attr("skipped")
        ),
        ("3", "1", "0", "0")
    );
    let names: Vec<&str> = a
        .cases
        .iter()
        .map(|c| c.iter().find(|(k, _)| k == "name").unwrap().1.as_str())
        .collect();
    assert_eq!(names, ["res.status eq 200", "a<b & \"c\"", "ok"]);
    assert_eq!(
        a.cases[0].iter().find(|(k, _)| k == "classname").unwrap().1,
        "a"
    );
    assert_eq!(a.failures, ["expected 1 to equal 2"]);

    let b = &suites[1];
    assert_eq!((b.attr("tests"), b.attr("errors")), ("1", "1"));
    assert_eq!(
        b.cases[0].iter().find(|(k, _)| k == "name").unwrap().1,
        "Test suite has no errors"
    );
    assert_eq!(b.errors.len(), 1);

    let c = &suites[2];
    assert_eq!(c.attr("skipped"), "1");
    assert_eq!(c.skipped, ["Request Skipped"]);
}

#[tokio::test]
async fn json_report_counts() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let json_path = dir.path().join("results.json");
    Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(dir.path())
        .arg("--reporter-json")
        .arg(&json_path)
        .assert()
        .code(1);
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json_path).unwrap()).unwrap();
    // Bruno writes one entry per iteration.
    assert_eq!(report.as_array().unwrap().len(), 1);
    let report = &report[0];
    assert_eq!(report["iterationIndex"], 0);
    let s = &report["summary"];
    assert_eq!(s["totalRequests"], 3);
    assert_eq!(s["passedRequests"], 0);
    assert_eq!(s["failedRequests"], 1);
    assert_eq!(s["errorRequests"], 1);
    assert_eq!(s["skippedRequests"], 1);
    assert_eq!(
        (
            s["totalTests"].clone(),
            s["passedTests"].clone(),
            s["failedTests"].clone()
        ),
        (2.into(), 1.into(), 1.into())
    );
    assert_eq!(
        (s["totalAssertions"].clone(), s["passedAssertions"].clone()),
        (1.into(), 1.into())
    );
    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    // As in Bruno, `status` says the request completed; its failed test is in the lists.
    assert_eq!(results[0]["status"], "pass");
    assert_eq!(results[0]["test"]["filename"], "a.bru");
    assert_eq!(results[0]["response"]["status"], 200);
    assert_eq!(results[0]["testResults"][1]["description"], "ok");
    assert_eq!(results[0]["assertionResults"][0]["lhsExpr"], "res.status");
    assert_eq!(results[1]["status"], "error");
    assert_eq!(results[2]["status"], "skipped");
}

#[tokio::test]
async fn cli_prints_tests_and_summary() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let out = Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(dir.path())
        .arg("--verbose")
        .assert()
        .code(1);
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).into_owned();
    for expected in [
        "✗ GET     a  200",
        "    ✓ assert res.status: eq 200",
        "    ✓ ok",
        "    ✗ a<b & \"c\": expected 1 to equal 2",
        "    │ from a",
        "- c.bru: request skipped via pre-request script",
        "Requests: 0 passed, 2 failed, 1 skipped",
        "Tests: 1/2",
        "Asserts: 1/1",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?} in:\n{stdout}"
        );
    }
}

#[tokio::test]
async fn console_hidden_without_verbose() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let out = Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(dir.path())
        .assert()
        .code(1);
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).into_owned();
    assert!(!stdout.contains("from a"), "{stdout}");
}

#[tokio::test]
async fn missing_report_directory_is_a_setup_error() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(dir.path().join("c.bru"))
        .arg("--reporter-junit")
        .arg(dir.path().join("nope/results.xml"))
        .assert()
        .code(2);
}

#[tokio::test]
async fn script_limits_flags() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    write(
        dir.path(),
        "slow.bru",
        &request(
            "slow",
            4,
            &format!("{}/slow", server.uri()),
            "\nscript:pre-request {\n  while (true) {}\n}\n",
        ),
    );
    let out = Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(dir.path().join("slow.bru"))
        .arg("--script-timeout-ms")
        .arg("200")
        .assert()
        .code(1);
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).into_owned();
    assert!(stdout.contains("script timed out after 200 ms"), "{stdout}");
}

#[tokio::test]
async fn junit_reports_post_response_script_errors_like_bruno() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"pe","type":"collection"}"#,
    );
    write(
        root,
        "01-a.bru",
        &request(
            "a",
            1,
            &format!("{}/a", server.uri()),
            "\nscript:post-response {\n  throw new Error('post <boom> ]]>');\n}\n",
        ),
    );
    let junit = root.join("junit.xml");
    Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(root)
        .arg("--reporter-junit")
        .arg(&junit)
        .assert()
        .code(1);
    let suites = parse_junit(&fs::read_to_string(&junit).unwrap());
    let a = &suites[0];
    assert_eq!((a.attr("tests"), a.attr("failures")), ("1", "1"));
    assert_eq!(
        a.cases[0].iter().find(|(k, _)| k == "name").unwrap().1,
        "Post-Response Script Error"
    );
    assert_eq!(a.failures, ["post <boom> ]]>"]);
}

#[tokio::test]
async fn requests_that_never_ran_are_named_by_meta_name() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"nm","type":"collection"}"#,
    );
    write(
        root,
        "01-first.bru",
        &request(
            "first",
            1,
            &format!("{}/a", server.uri()),
            "\nscript:post-response {\n  bru.runner.stopExecution();\n}\n",
        ),
    );
    write(
        root,
        "04-r4.bru",
        &request("r4", 4, &format!("{}/b", server.uri()), ""),
    );
    let json_path = root.join("results.json");
    Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(root)
        .arg("--reporter-json")
        .arg(&json_path)
        .assert()
        .success();
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json_path).unwrap()).unwrap();
    let results = report[0]["results"].as_array().unwrap();
    assert_eq!(results[1]["status"], "skipped");
    assert_eq!(results[1]["name"], "r4");
}

#[tokio::test]
async fn junit_pre_request_errors_match_bruno() {
    // Bruno 4.0 for a pre-request script that throws: the suite is named by meta name,
    // the error message is the thrown message alone, and time is the real duration.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"pre","type":"collection"}"#,
    );
    write(
        root,
        "1-access-control/P1-07.bru",
        &request(
            "P1-07 · [token] · 401 token_invalid",
            1,
            "http://127.0.0.1:9/never",
            "\nscript:pre-request {\n  await bru.sleep(80);\n  throw new Error('Request failed with status code 401');\n}\n",
        ),
    );
    let junit = root.join("junit.xml");
    let json_path = root.join("results.json");
    Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(root)
        .arg("--reporter-junit")
        .arg(&junit)
        .arg("--reporter-json")
        .arg(&json_path)
        .assert()
        .code(1);
    let suites = parse_junit(&fs::read_to_string(&junit).unwrap());
    let s = &suites[0];
    assert_eq!(s.attr("name"), "P1-07 · [token] · 401 token_invalid");
    assert_eq!(s.attr("file"), "1-access-control/P1-07.bru");
    assert_eq!(
        (s.attr("errors"), s.attr("failures"), s.attr("tests")),
        ("1", "0", "1")
    );
    assert_eq!(s.errors, ["Request failed with status code 401"]);
    let time: f64 = s.attr("time").parse().unwrap();
    assert!(time >= 0.08, "real duration, got {time}");
    let case = &s.cases[0];
    let get = |k: &str| case.iter().find(|(n, _)| n == k).unwrap().1.clone();
    assert_eq!(get("name"), "Test suite has no errors");
    assert_eq!(get("classname"), "1-access-control/P1-07");
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json_path).unwrap()).unwrap();
    let r = &report[0]["results"][0];
    assert_eq!(r["name"], "P1-07 · [token] · 401 token_invalid");
    assert_eq!(r["error"], "Request failed with status code 401");
}

#[tokio::test]
async fn accepts_bruno_run_flags_used_by_existing_scripts() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let root = dir.path();
    // `bru run bruno -r --env Dev --sandbox safe --delay 0`: -r and --sandbox safe are
    // what Plover always does.
    Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(root.join("a.bru"))
        .args(["-r", "--sandbox", "safe", "--delay", "0"])
        .assert()
        .code(1); // a.bru has one failing test
    Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(root)
        .arg("--recursive")
        .assert()
        .code(1);
    let out = Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(root)
        .args(["--sandbox", "developer"])
        .assert()
        .code(2);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).into_owned();
    assert!(stderr.contains("safe"), "{stderr}");
}

#[tokio::test]
async fn html_report_is_self_contained_and_escaped() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let root = dir.path();
    let html_path = root.join("report.html");
    Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(root)
        .arg("--reporter-html")
        .arg(&html_path)
        .assert()
        .code(1);
    let html = fs::read_to_string(&html_path).unwrap();
    assert!(html.starts_with("<!doctype html>"));
    // Summary (a fails a test, b cannot connect, c is skipped) and every request.
    for needle in [
        "Requests",
        "0 passed",
        "2 failed",
        "1 skipped",
        ">a<",
        ">b<",
        ">c<",
    ] {
        assert!(html.contains(needle), "missing {needle}");
    }
    // The failing test's name is escaped, and its message is shown.
    assert!(html.contains("a&lt;b &amp; &quot;c&quot;"));
    assert!(html.contains("expected 1 to equal 2"));
    // Offline: no external resources and no script needed to read it.
    for forbidden in ["<script", "<link", "src=\"http", "href=\"http", "@import"] {
        assert!(!html.contains(forbidden), "found {forbidden}");
    }
}

/// login returns a token in its body; me sends it as `Authorization: Bearer` and returns
/// `{"id":1}` with header `x-r`.
async fn detail_collection(server: &MockServer) -> tempfile::TempDir {
    use wiremock::matchers::path;
    Mock::given(path("/login"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"token": "tok-AAAABBBBCCCC"})),
        )
        .mount(server)
        .await;
    Mock::given(path("/me"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-r", "y")
                .set_body_json(serde_json::json!({"id": 1})),
        )
        .mount(server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"detail","type":"collection"}"#,
    );
    write(
        root,
        "login.bru",
        &request(
            "login",
            1,
            &format!("{}/login", server.uri()),
            "\nscript:post-response {\n  bru.setVar('token', res.body.token);\n}\n",
        ),
    );
    write(
        root,
        "me.bru",
        &request(
            "me",
            2,
            &format!("{}/me", server.uri()),
            "\nheaders {\n  Authorization: Bearer {{token}}\n  x-q: 1\n}\n",
        ),
    );
    write(
        root,
        "down.bru",
        &request("down", 3, "http://127.0.0.1:9/down", ""),
    );
    dir
}

fn plv_run(root: &Path, extra: &[&str]) -> assert_cmd::assert::Assert {
    let state = tempfile::tempdir().unwrap();
    Command::cargo_bin("plv")
        .unwrap()
        .env("PLOVER_STATE_DIR", state.path())
        .env_remove("CI")
        .arg("run")
        .arg(root)
        .args(extra)
        .assert()
}

#[tokio::test]
async fn json_report_has_bruno_request_and_response() {
    let server = MockServer::start().await;
    let dir = detail_collection(&server).await;
    let out = dir.path().join("r.json");
    plv_run(dir.path(), &["--reporter-json", out.to_str().unwrap()]).code(1);
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    let results = v[0]["results"].as_array().unwrap();
    let me = &results[1];
    for key in ["method", "url", "headers", "data"] {
        assert!(me["request"].get(key).is_some(), "request.{key}");
    }
    for key in [
        "status",
        "statusText",
        "headers",
        "data",
        "url",
        "responseTime",
        "duration",
        "size",
    ] {
        assert!(me["response"].get(key).is_some(), "response.{key}");
    }
    assert!(me["request"]["headers"].is_object());
    assert_eq!(me["request"]["headers"]["x-q"], "1");
    assert_eq!(me["response"]["data"], serde_json::json!({"id": 1}));
    assert_eq!(me["response"]["headers"]["x-r"], "y");
    assert_eq!(me["response"]["statusText"], "OK");
    let down = &results[2];
    assert_eq!(down["response"]["status"], "error");
    assert_eq!(down["response"]["size"], 0);
}

#[tokio::test]
async fn reports_are_masked() {
    let server = MockServer::start().await;
    let dir = detail_collection(&server).await;
    let (json, html, xml) = (
        dir.path().join("r.json"),
        dir.path().join("r.html"),
        dir.path().join("r.xml"),
    );
    plv_run(
        dir.path(),
        &[
            "--reporter-json",
            json.to_str().unwrap(),
            "--reporter-html",
            html.to_str().unwrap(),
            "--reporter-junit",
            xml.to_str().unwrap(),
        ],
    )
    .code(1);
    for path in [&json, &html] {
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("***"), "{}", path.display());
        assert!(!text.contains("tok-AAAABBBBCCCC"), "{}", path.display());
    }
    assert!(
        !fs::read_to_string(&xml)
            .unwrap()
            .contains("tok-AAAABBBBCCCC")
    );
}

#[tokio::test]
async fn skip_flags_match_bruno() {
    let server = MockServer::start().await;
    let dir = detail_collection(&server).await;
    let out = dir.path().join("r.json");
    let read = |out: &Path| -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(out).unwrap()).unwrap()
    };
    plv_run(
        dir.path(),
        &[
            "--reporter-json",
            out.to_str().unwrap(),
            "--reporter-skip-all-headers",
        ],
    )
    .code(1);
    let me = &read(&out)[0]["results"][1];
    assert_eq!(me["request"]["headers"], serde_json::json!({}));
    assert_eq!(me["response"]["headers"], serde_json::json!({}));
    plv_run(
        dir.path(),
        &[
            "--reporter-json",
            out.to_str().unwrap(),
            "--reporter-skip-headers",
            "X-R",
        ],
    )
    .code(1);
    let me = &read(&out)[0]["results"][1];
    assert!(me["response"]["headers"].get("x-r").is_none());
    assert_eq!(me["request"]["headers"]["x-q"], "1");
    plv_run(
        dir.path(),
        &[
            "--reporter-json",
            out.to_str().unwrap(),
            "--reporter-skip-body",
        ],
    )
    .code(1);
    let me = &read(&out)[0]["results"][1];
    assert!(me["request"].get("data").is_none());
    assert!(me["response"].get("data").is_none());
}

#[tokio::test]
async fn html_has_details_and_no_script() {
    let server = MockServer::start().await;
    let dir = detail_collection(&server).await;
    let out = dir.path().join("r.html");
    plv_run(dir.path(), &["--reporter-html", out.to_str().unwrap()]).code(1);
    let html = fs::read_to_string(&out).unwrap();
    // Every result that was sent: login, me, and down (sent, no response).
    assert_eq!(html.matches("<details>").count(), 3);
    assert!(html.contains("<code>x-r</code>"));
    assert!(!html.contains("<script"));
}
