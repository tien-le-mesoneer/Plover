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
    Command::cargo_bin("plv").unwrap()
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

/// Bruno CLI 4.3 writes script changes to the selected environment and collection.bru.
#[tokio::test(flavor = "multi_thread")]
async fn run_persists_script_variable_changes() {
    let server = MockServer::start().await;
    Mock::given(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let dir = collection(&server.uri());
    write(
        dir.path(),
        "ok.bru",
        &format!(
            "{}\nscript:post-response {{\n  bru.setEnvVar('token', 'rotated');\n  bru.setCollectionVar('cv', 1);\n}}\n",
            request("ok", "/ok")
        ),
    );
    bru_rs()
        .args(["run", "--env", "local"])
        .arg(dir.path().join("ok.bru"))
        .assert()
        .code(0);
    assert_eq!(
        fs::read_to_string(dir.path().join("environments/local.bru")).unwrap(),
        format!("vars {{\n  host: {}\n  token: rotated\n}}\n", server.uri())
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("collection.bru")).unwrap(),
        "vars:pre-request {\n  @number\n  cv: 1\n}\n"
    );
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

#[test]
fn serve_prints_url_and_opens_collection() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::{Command as StdCommand, Stdio};

    let dir = collection("http://127.0.0.1:9");
    write(dir.path(), "a.bru", &request("a", "/a"));
    let mut child = StdCommand::new(assert_cmd::cargo::cargo_bin("plv"))
        .args(["serve", "--no-open", "--port", "0"])
        .arg(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let url = line
        .trim()
        .strip_prefix("plv listening on ")
        .unwrap_or_else(|| panic!("unexpected first line: {line}"))
        .to_owned();
    let (base, token) = url.split_once("/#token=").unwrap();
    let addr = base.strip_prefix("http://").unwrap();
    assert!(addr.starts_with("127.0.0.1:"));
    assert_eq!(token.len(), 64);

    let get = |path: &str| {
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    };
    let collections = get("/api/collections");
    assert!(collections.starts_with("HTTP/1.1 200"), "{collections}");
    let body = collections.split("\r\n\r\n").nth(1).unwrap();
    let list: serde_json::Value = serde_json::from_str(body).unwrap();
    let id = list[0]["id"].as_str().unwrap();
    let tree = get(&format!("/api/collections/{id}/tree"));
    assert!(tree.starts_with("HTTP/1.1 200"), "{tree}");
    assert!(tree.contains(r#""id":"a.bru""#), "{tree}");
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn serve_bad_collection_path_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    bru_rs()
        .args(["serve", "--no-open", "--port", "0"])
        .arg(dir.path())
        .assert()
        .code(2);
}
