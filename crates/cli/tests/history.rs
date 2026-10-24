//! `plv run` saving to the run history, and `plv runs`. Every test uses its own state
//! directory.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn request(name: &str, seq: u32, url: &str) -> String {
    format!(
        "meta {{\n  name: {name}\n  type: http\n  seq: {seq}\n}}\n\nget {{\n  url: {url}\n  body: none\n  auth: none\n}}\n"
    )
}

/// One passing request (`ok`), and `bad` that returns 500 and fails its assert when
/// `failing`.
async fn collection(server: &MockServer, failing: bool) -> tempfile::TempDir {
    Mock::given(path("/ok"))
        .respond_with(ResponseTemplate::new(200).set_body_string("fine"))
        .mount(server)
        .await;
    Mock::given(path("/bad"))
        .respond_with(ResponseTemplate::new(500))
        .mount(server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"hist","type":"collection"}"#,
    );
    write(
        root,
        "ok.bru",
        &request("ok", 1, &format!("{}/ok", server.uri())),
    );
    if failing {
        write(
            root,
            "bad.bru",
            &format!(
                "{}\nassert {{\n  res.status: eq 200\n}}\n",
                request("bad", 2, &format!("{}/bad", server.uri()))
            ),
        );
    }
    dir
}

fn plv(state: &Path) -> Command {
    let mut cmd = Command::cargo_bin("plv").unwrap();
    cmd.env("PLOVER_STATE_DIR", state).env_remove("CI");
    cmd
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The ID from `Saved run <id>` on stderr.
fn saved_id(out: &std::process::Output) -> Option<String> {
    stderr(out)
        .lines()
        .find_map(|l| l.strip_prefix("Saved run "))
        .map(|id| id.trim().to_owned())
}

#[tokio::test]
async fn run_saves_history() {
    let server = MockServer::start().await;
    let dir = collection(&server, false).await;
    let state = tempfile::tempdir().unwrap();
    let out = plv(state.path())
        .arg("run")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let id = saved_id(&out).expect("Saved run on stderr");
    assert!(!stdout(&out).contains("Saved run"));
    let list = plv(state.path())
        .arg("runs")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(list.status.success());
    let listed = stdout(&list);
    assert!(listed.contains(&id), "{listed}");
    assert!(listed.contains("plv"));
    // History lives in the state directory, never in the collection.
    assert!(state.path().join("runs").is_dir());
    assert!(!dir.path().join("runs").exists());
}

#[tokio::test]
async fn ci_disables_history() {
    let server = MockServer::start().await;
    let dir = collection(&server, false).await;
    let saved = |ci: Option<&str>, flags: &[&str]| {
        let state = tempfile::tempdir().unwrap();
        let mut cmd = plv(state.path());
        if let Some(ci) = ci {
            cmd.env("CI", ci);
        }
        let out = cmd.arg("run").arg(dir.path()).args(flags).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        saved_id(&out).is_some()
    };
    assert!(!saved(Some("true"), &[]));
    assert!(saved(Some("true"), &["--history"]));
    assert!(!saved(None, &["--no-history"]));
    assert!(saved(Some("0"), &[]));
    assert!(saved(Some("false"), &[]));
    assert!(saved(None, &[]));
    let state = tempfile::tempdir().unwrap();
    plv(state.path())
        .arg("run")
        .arg(dir.path())
        .args(["--history", "--no-history"])
        .assert()
        .code(2);
}

#[tokio::test]
async fn show_matches_live_report() {
    let server = MockServer::start().await;
    let dir = collection(&server, true).await;
    let state = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.json"), dir.path().join("b.json"));
    let out = plv(state.path())
        .arg("run")
        .arg(dir.path())
        .arg("--reporter-json")
        .arg(&a)
        .output()
        .unwrap();
    let id = saved_id(&out).unwrap();
    let show = plv(state.path())
        .args(["runs", "show", &id, "--reporter-json"])
        .arg(&b)
        .output()
        .unwrap();
    assert!(show.status.success(), "{}", stderr(&show));
    assert_eq!(
        fs::read_to_string(&a).unwrap(),
        fs::read_to_string(&b).unwrap()
    );
    let printed = stdout(&show);
    assert!(printed.contains(&format!("Run {id}")), "{printed}");
    assert!(
        printed.contains("Requests: 1 passed, 1 failed"),
        "{printed}"
    );
}

#[tokio::test]
async fn show_after_collection_deleted() {
    let server = MockServer::start().await;
    let dir = collection(&server, false).await;
    let state = tempfile::tempdir().unwrap();
    let out = plv(state.path())
        .arg("run")
        .arg(dir.path())
        .output()
        .unwrap();
    let id = saved_id(&out).unwrap();
    let report_dir = tempfile::tempdir().unwrap();
    let html = report_dir.path().join("r.html");
    drop(dir);
    plv(state.path())
        .args(["runs", "show", &id, "--reporter-html"])
        .arg(&html)
        .assert()
        .code(0);
    assert!(fs::read_to_string(&html).unwrap().contains("hist"));
}

#[tokio::test]
async fn unwritable_history_keeps_exit_code() {
    let server = MockServer::start().await;
    let dir = collection(&server, true).await;
    let not_a_dir = dir.path().join("state-file");
    fs::write(&not_a_dir, "x").unwrap();
    let json = dir.path().join("r.json");
    let out = plv(&not_a_dir)
        .arg("run")
        .arg(dir.path())
        .arg("--reporter-json")
        .arg(&json)
        .output()
        .unwrap();
    let state = tempfile::tempdir().unwrap();
    let without = plv(state.path())
        .arg("run")
        .arg(dir.path())
        .arg("--no-history")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), without.status.code());
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("warning: cannot save run history"),
        "{}",
        stderr(&out)
    );
    assert!(json.is_file());
}

#[tokio::test]
async fn failed_run_is_saved_with_exit_1() {
    let server = MockServer::start().await;
    let dir = collection(&server, true).await;
    let state = tempfile::tempdir().unwrap();
    let out = plv(state.path())
        .arg("run")
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let id = saved_id(&out).unwrap();
    let list = stdout(
        &plv(state.path())
            .arg("runs")
            .arg(dir.path())
            .output()
            .unwrap(),
    );
    assert!(list.contains(&id));
    assert!(list.contains("1/1/0"), "{list}");
}

#[tokio::test]
async fn clear_requires_yes() {
    let server = MockServer::start().await;
    let dir = collection(&server, false).await;
    let state = tempfile::tempdir().unwrap();
    plv(state.path())
        .arg("run")
        .arg(dir.path())
        .assert()
        .code(0);
    plv(state.path())
        .args(["runs", "clear"])
        .arg(dir.path())
        .write_stdin("")
        .assert()
        .code(2);
    plv(state.path())
        .args(["runs", "clear"])
        .arg(dir.path())
        .arg("--yes")
        .assert()
        .code(0);
    let list = stdout(
        &plv(state.path())
            .arg("runs")
            .arg(dir.path())
            .output()
            .unwrap(),
    );
    assert!(list.contains("No runs saved for hist"), "{list}");
}

#[tokio::test]
async fn concurrent_plv_runs() {
    let server = MockServer::start().await;
    let dir = collection(&server, false).await;
    let state = tempfile::tempdir().unwrap();
    let spawn = || {
        std::process::Command::new(assert_cmd::cargo::cargo_bin("plv"))
            .env("PLOVER_STATE_DIR", state.path())
            .env_remove("CI")
            .arg("run")
            .arg(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    };
    let (a, b) = (spawn(), spawn());
    let ids: Vec<String> = [a, b]
        .into_iter()
        .map(|c| saved_id(&c.wait_with_output().unwrap()).unwrap())
        .collect();
    assert_ne!(ids[0], ids[1]);
    let list = stdout(
        &plv(state.path())
            .arg("runs")
            .arg(dir.path())
            .output()
            .unwrap(),
    );
    for id in &ids {
        assert!(list.contains(id));
        plv(state.path())
            .args(["runs", "show", id])
            .assert()
            .code(0);
    }
}

#[tokio::test]
async fn unknown_or_malformed_id() {
    let state = tempfile::tempdir().unwrap();
    let bad = plv(state.path())
        .args(["runs", "show", "../x"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
    assert!(stderr(&bad).contains("invalid run ID"), "{}", stderr(&bad));
    let unknown = plv(state.path())
        .args(["runs", "show", "19700102T000000Z-abcdef"])
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(2));
    assert!(
        stderr(&unknown).contains("run 19700102T000000Z-abcdef not found"),
        "{}",
        stderr(&unknown)
    );
}
