//! Phase 3 acceptance: the chained fixture collection passes through the CLI.

use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/phase3-chain")
}

#[tokio::test]
async fn phase3_chain_passes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login"))
        .and(body_json(json!({ "user": "ana", "password": "secret" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "token": "t-abc" })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/me"))
        .and(header("authorization", "Bearer t-abc"))
        .and(header("x-run", "chain"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "name": "Ana" })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/users"))
        .and(header("authorization", "Bearer t-abc"))
        .and(header("x-request-id", "req-t-abc"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": 42, "name": "Bo" })))
        .expect(1)
        .mount(&server)
        .await;
    let out_dir = tempfile::tempdir().unwrap();
    let junit = out_dir.path().join("junit.xml");
    let out = Command::cargo_bin("plv")
        .unwrap()
        .arg("run")
        .arg(fixture())
        .args(["--env", "local", "--env-var"])
        .arg(format!("host={}", server.uri()))
        .arg("--reporter-junit")
        .arg(&junit)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).into_owned();
    assert!(
        stdout.contains("Requests: 3 passed, 0 failed, 0 skipped · Tests: 5/5 · Asserts: 2/2"),
        "{stdout}"
    );
    let xml = std::fs::read_to_string(&junit).unwrap();
    assert_eq!(xml.matches("<testsuite ").count(), 3, "{xml}");
    assert_eq!(xml.matches("status=\"pass\"").count(), 7, "{xml}");
    assert!(!xml.contains("<failure"), "{xml}");
}
