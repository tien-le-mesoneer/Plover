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

/// `plv` with history kept in a temporary state directory, never the user's.
fn bru_rs() -> Command {
    static STATE: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let state = STATE.get_or_init(|| tempfile::tempdir().unwrap());
    let mut cmd = Command::cargo_bin("plv").unwrap();
    cmd.env("PLOVER_STATE_DIR", state.path());
    cmd
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

/// A running `plv serve` with its own state dir (tests never touch the real one).
struct Served {
    child: std::process::Child,
    addr: String,
    token: String,
}

impl Served {
    fn start(state_dir: &Path, path: Option<&Path>) -> Served {
        use std::io::{BufRead, BufReader};
        use std::process::{Command as StdCommand, Stdio};

        let mut cmd = StdCommand::new(assert_cmd::cargo::cargo_bin("plv"));
        cmd.args(["serve", "--no-open", "--port", "0"])
            .env("PLOVER_STATE_DIR", state_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(path) = path {
            cmd.arg(path);
        }
        let mut child = cmd.spawn().unwrap();
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
        let addr = base.strip_prefix("http://").unwrap().to_owned();
        Served {
            child,
            addr,
            token: token.to_owned(),
        }
    }

    fn get(&self, path: &str) -> String {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect(&self.addr).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
            self.addr, self.token
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    fn json(&self, path: &str) -> serde_json::Value {
        let response = self.get(path);
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap()).unwrap()
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn serve_prints_url_and_opens_collection() {
    let dir = collection("http://127.0.0.1:9");
    write(dir.path(), "a.bru", &request("a", "/a"));
    let state = tempfile::tempdir().unwrap();
    let served = Served::start(state.path(), Some(dir.path()));
    assert!(served.addr.starts_with("127.0.0.1:"));
    assert_eq!(served.token.len(), 64);
    let list = served.json("/api/collections");
    let id = list[0]["id"].as_str().unwrap();
    let tree = served.get(&format!("/api/collections/{id}/tree"));
    assert!(tree.starts_with("HTTP/1.1 200"), "{tree}");
    assert!(tree.contains(r#""id":"a.bru""#), "{tree}");
}

#[test]
fn serve_reopens_collections_from_the_last_run() {
    let dir = collection("http://127.0.0.1:9");
    let state = tempfile::tempdir().unwrap();
    let first = Served::start(state.path(), Some(dir.path()));
    let id = first.json("/api/collections")[0]["id"].clone();
    drop(first);
    assert!(state.path().join("state.json").is_file());

    let second = Served::start(state.path(), None);
    let list = second.json("/api/collections");
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
    assert_eq!(list[0]["id"], id);
    assert_eq!(list[0]["available"], true);
}

#[test]
fn serve_bad_collection_path_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    bru_rs()
        .args(["serve", "--no-open", "--port", "0"])
        .env("PLOVER_STATE_DIR", state.path())
        .arg(dir.path())
        .assert()
        .code(2);
}

/// A `--no-default-features` build has no web UI but keeps the API.
#[cfg(not(feature = "embed-ui"))]
#[test]
fn cli_only_build_serves_api_without_ui() {
    let state = tempfile::tempdir().unwrap();
    let served = Served::start(state.path(), None);
    let index = served.get("/");
    assert!(index.contains("CLI-only build"), "{index}");
    assert!(served.get("/api/health").starts_with("HTTP/1.1 200"));
}

#[test]
fn use_keychain_requires_env() {
    let dir = collection("http://127.0.0.1:9");
    let out = bru_rs()
        .args(["run", "--use-keychain"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--env"));
}
