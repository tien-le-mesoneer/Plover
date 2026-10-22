//! Test output, JUnit and JSON reports from `bru-rs run` (Bruno CLI reporters:
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
    Command::cargo_bin("bru-rs")
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
    Command::cargo_bin("bru-rs")
        .unwrap()
        .arg("run")
        .arg(dir.path())
        .arg("--reporter-json")
        .arg(&json_path)
        .assert()
        .code(1);
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json_path).unwrap()).unwrap();
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
    assert_eq!(results[0]["status"], "fail");
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
    let out = Command::cargo_bin("bru-rs")
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
    let out = Command::cargo_bin("bru-rs")
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
    Command::cargo_bin("bru-rs")
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
    let out = Command::cargo_bin("bru-rs")
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
