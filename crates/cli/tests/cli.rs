use std::fs;
use std::path::Path;

use assert_cmd::Command;
use wiremock::matchers::{header, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn request(name: &str, url_path: &str) -> String {
    format!(
        "meta {{\n  name: {name}\n  type: http\n  seq: 1\n}}\n\nget {{\n  url: {{{{host}}}}{url_path}\n  auth: bearer\n}}\n\nauth:bearer {{\n  token: {{{{token}}}}\n}}\n"
    )
}

fn collection(host: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "bruno.json",
        r#"{"version":"1","name":"t","type":"collection"}"#,
    );
    write(
        dir.path(),
        "environments/local.bru",
        &format!("vars {{\n  host: {host}\n  token: env\n}}\n"),
    );
    dir
}

fn bru_rs() -> Command {
    Command::cargo_bin("bru-rs").unwrap()
}

async fn server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(path("/err"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    server
}

#[tokio::test(flavor = "multi_thread")]
async fn exit_codes() {
    let server = server().await;
    let dir = collection(&server.uri());
    let root = dir.path();
    write(root, "ok.bru", &request("ok", "/ok"));
    let out = bru_rs()
        .args(["run", "--env", "local"])
        .arg(root)
        .assert()
        .code(0);
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).into_owned();
    assert!(
        stdout.contains("✓ GET") && stdout.contains("1 passed, 0 failed, 0 skipped"),
        "{stdout}"
    );

    write(root, "err.bru", &request("err", "/err"));
    bru_rs()
        .args(["run", "--env", "local"])
        .arg(root)
        .assert()
        .code(0);

    let refused = collection("http://127.0.0.1:9");
    write(refused.path(), "ok.bru", &request("ok", "/ok"));
    bru_rs()
        .args(["run", "--env", "local"])
        .arg(refused.path())
        .assert()
        .code(1);

    bru_rs()
        .args(["run", "--env", "nope"])
        .arg(root)
        .assert()
        .code(2);
    bru_rs()
        .args(["run"])
        .arg(root.join("missing.bru"))
        .assert()
        .code(2);
    bru_rs()
        .args(["run", "--env-var", "novalue"])
        .arg(root)
        .assert()
        .code(2);
}

#[tokio::test(flavor = "multi_thread")]
async fn env_var_flag_overrides() {
    let server = MockServer::start().await;
    Mock::given(path("/ok"))
        .and(header("authorization", "Bearer xyz"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let dir = collection(&server.uri());
    write(dir.path(), "ok.bru", &request("ok", "/ok"));
    bru_rs()
        .args(["run", "--env", "local", "--env-var", "token=xyz"])
        .arg(dir.path().join("ok.bru"))
        .assert()
        .code(0);
}

#[tokio::test(flavor = "multi_thread")]
async fn broken_file_exit_zero() {
    let server = server().await;
    let dir = collection(&server.uri());
    write(dir.path(), "ok.bru", &request("ok", "/ok"));
    write(dir.path(), "broken.bru", "get {\n");
    let out = bru_rs()
        .args(["run", "--env", "local"])
        .arg(dir.path())
        .assert()
        .code(0);
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).into_owned();
    assert!(stdout.contains("- broken.bru: parse error"), "{stdout}");
    assert!(stdout.contains("1 passed, 0 failed, 1 skipped"), "{stdout}");
}
