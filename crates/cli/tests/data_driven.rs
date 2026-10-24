//! `plv run --csv-file-path / --json-file-path / --iteration-count / --parallel`.
//! Every test uses its own state directory and synthetic data only.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use wiremock::matchers::path_regex;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

/// Two requests: `a` GET `/a/{{user}}`, `b` POST `/b` with a JSON body holding the
/// password; `b` asserts `res.status: eq 200`.
async fn collection(server: &MockServer) -> tempfile::TempDir {
    Mock::given(path_regex("^/a/bad$"))
        .respond_with(ResponseTemplate::new(500))
        .mount(server)
        .await;
    Mock::given(path_regex(".*"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "bruno.json",
        r#"{"version":"1","name":"data","type":"collection"}"#,
    );
    write(
        root,
        "a.bru",
        &format!(
            "meta {{\n  name: a\n  type: http\n  seq: 1\n}}\n\nget {{\n  url: {}/a/{{{{user}}}}\n  body: none\n  auth: none\n}}\n\nassert {{\n  res.status: eq 200\n}}\n",
            server.uri()
        ),
    );
    write(
        root,
        "b.bru",
        &format!(
            "meta {{\n  name: b\n  type: http\n  seq: 2\n}}\n\npost {{\n  url: {}/b\n  body: json\n  auth: none\n}}\n\nbody:json {{\n  {{\"u\":\"{{{{user}}}}\",\"p\":\"{{{{password}}}}\"}}\n}}\n",
            server.uri()
        ),
    );
    dir
}

const CSV: &str = "user,password\nalice,pw-SECRET-1\nbob,pw-SECRET-2\ncarol,pw-SECRET-3\n";

fn plv(state: &Path) -> Command {
    let mut cmd = Command::cargo_bin("plv").unwrap();
    cmd.env("PLOVER_STATE_DIR", state).env_remove("CI");
    cmd
}

fn out(o: &std::process::Output) -> (String, String) {
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

async fn hits(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_owned())
        .collect()
}

#[tokio::test]
async fn csv_runs_each_row() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let state = tempfile::tempdir().unwrap();
    let data = dir.path().join("rows.csv");
    fs::write(&data, CSV).unwrap();
    let report = state.path().join("r.json");
    let o = plv(state.path())
        .arg("run")
        .arg(dir.path())
        .arg("--csv-file-path")
        .arg(&data)
        .arg("--reporter-json")
        .arg(&report)
        .output()
        .unwrap();
    let (stdout, stderr) = out(&o);
    assert_eq!(o.status.code(), Some(0), "{stdout}\n{stderr}");
    for n in 1..=3 {
        assert!(stdout.contains(&format!("Iteration: {n}")), "{stdout}");
    }
    assert!(stdout.contains("Iterations: 3"), "{stdout}");
    assert_eq!(
        hits(&server).await,
        ["/a/alice", "/b", "/a/bob", "/b", "/a/carol", "/b"]
    );
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 3);
    // The password column is masked in the report's request bodies.
    let text = json.to_string();
    assert!(!text.contains("pw-SECRET"), "{text}");
}

#[tokio::test]
async fn json_file_works() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let state = tempfile::tempdir().unwrap();
    let data = dir.path().join("rows.json");
    fs::write(
        &data,
        r#"[{"user":"x1","password":"pw-1234"},{"user":"x2"}]"#,
    )
    .unwrap();
    let o = plv(state.path())
        .arg("run")
        .arg(dir.path())
        .arg("--json-file-path")
        .arg(&data)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{:?}", out(&o));
    assert_eq!(hits(&server).await, ["/a/x1", "/b", "/a/x2", "/b"]);
}

#[tokio::test]
async fn iteration_count_without_file() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let state = tempfile::tempdir().unwrap();
    let o = plv(state.path())
        .args(["run", "--iteration-count", "3", "--env-var", "user=u"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{:?}", out(&o));
    assert_eq!(hits(&server).await.len(), 6);
    let o = plv(state.path())
        .args(["run", "--iteration-count", "0"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
}

#[tokio::test]
async fn file_wins_over_count() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let state = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("d.csv"), "user\nu1\nu2\n").unwrap();
    let o = plv(state.path())
        .current_dir(dir.path())
        .args([
            "run",
            ".",
            "--csv-file-path",
            "d.csv",
            "--iteration-count",
            "9",
        ])
        .output()
        .unwrap();
    let (_, stderr) = out(&o);
    assert!(
        stderr.contains("--iteration-count ignored: d.csv has 2 rows"),
        "{stderr}"
    );
    assert_eq!(hits(&server).await.len(), 4);
}

#[tokio::test]
async fn loud_failures() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let state = tempfile::tempdir().unwrap();
    let d = dir.path();
    fs::write(d.join("header.csv"), "user,password\n").unwrap();
    fs::write(d.join("empty.json"), "[]").unwrap();
    fs::write(d.join("object.json"), "{}").unwrap();
    fs::write(d.join("short.csv"), "a,b\n1,2\n3\n").unwrap();
    fs::write(d.join("big.csv"), "a".repeat((16 << 20) + 1)).unwrap();
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["--csv-file-path", "missing.csv"], "missing.csv"),
        (
            vec!["--csv-file-path", "header.csv"],
            "header.csv: file has no rows",
        ),
        (
            vec!["--json-file-path", "empty.json"],
            "empty.json: file has no rows",
        ),
        (
            vec!["--json-file-path", "object.json"],
            "object.json: the top level",
        ),
        (
            vec!["--csv-file-path", "short.csv"],
            "short.csv:3: expected 2 fields, found 1",
        ),
        (
            vec!["--csv-file-path", "big.csv"],
            "big.csv: file is larger than 16 MiB",
        ),
        (
            vec![
                "--csv-file-path",
                "short.csv",
                "--json-file-path",
                "empty.json",
            ],
            "cannot be used with",
        ),
    ];
    for (flags, message) in cases {
        let o = plv(state.path())
            .current_dir(d)
            .args(["run", "."])
            .args(&flags)
            .output()
            .unwrap();
        let (_, stderr) = out(&o);
        assert_eq!(o.status.code(), Some(2), "{flags:?}: {stderr}");
        assert!(stderr.contains(message), "{flags:?}: {stderr}");
    }
    assert!(hits(&server).await.is_empty());
}

#[tokio::test]
async fn parallel_warns() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let state = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("d.csv"), "user\nu1\nu2\n").unwrap();
    let o = plv(state.path())
        .current_dir(dir.path())
        .args(["run", ".", "--csv-file-path", "d.csv", "--parallel"])
        .output()
        .unwrap();
    let (_, stderr) = out(&o);
    assert!(
        stderr.contains("--parallel is not supported yet; iterations run one after another"),
        "{stderr}"
    );
    assert_eq!(hits(&server).await, ["/a/u1", "/b", "/a/u2", "/b"]);
}

#[tokio::test]
async fn verbose_row_masked() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let state = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("d.csv"), CSV).unwrap();
    let o = plv(state.path())
        .current_dir(dir.path())
        .args(["run", ".", "--csv-file-path", "d.csv", "--verbose"])
        .output()
        .unwrap();
    let (stdout, _) = out(&o);
    assert!(stdout.contains("\"password\": \"***\""), "{stdout}");
    assert!(stdout.contains("\"user\": \"alice\""), "{stdout}");
    assert!(!stdout.contains("pw-SECRET"), "{stdout}");
}

#[tokio::test]
async fn exit_one_if_any_iteration_fails() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let state = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("d.csv"), "user\nok\nbad\n").unwrap();
    let o = plv(state.path())
        .current_dir(dir.path())
        .args(["run", ".", "--csv-file-path", "d.csv"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1), "{:?}", out(&o));
}

#[tokio::test]
async fn relative_path_from_cwd() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let state = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    fs::write(cwd.path().join("rows.csv"), "user\nfrom-cwd\n").unwrap();
    fs::write(dir.path().join("rows.csv"), "user\nfrom-collection\n").unwrap();
    let o = plv(state.path())
        .current_dir(cwd.path())
        .arg("run")
        .arg(dir.path())
        .args(["--csv-file-path", "rows.csv"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{:?}", out(&o));
    assert_eq!(hits(&server).await[0], "/a/from-cwd");
}

#[tokio::test]
async fn history_keeps_iterations() {
    let server = MockServer::start().await;
    let dir = collection(&server).await;
    let state = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("d.csv"), CSV).unwrap();
    let live = state.path().join("live.json");
    let o = plv(state.path())
        .current_dir(dir.path())
        .args(["run", ".", "--csv-file-path", "d.csv", "--reporter-json"])
        .arg(&live)
        .output()
        .unwrap();
    let (_, stderr) = out(&o);
    let id = stderr
        .lines()
        .find_map(|l| l.strip_prefix("Saved run "))
        .unwrap()
        .trim()
        .to_owned();
    let shown = state.path().join("shown.json");
    let o = plv(state.path())
        .current_dir(dir.path())
        .args(["runs", "show", &id, "--reporter-json"])
        .arg(&shown)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{:?}", out(&o));
    assert_eq!(fs::read(&live).unwrap(), fs::read(&shown).unwrap());
    // The stored record never holds the passwords.
    let stored: String = walk(state.path());
    assert!(!stored.contains("pw-SECRET"));
}

/// Every file under `dir` concatenated (the history store).
fn walk(dir: &Path) -> String {
    let mut out = String::new();
    for entry in fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            out.push_str(&walk(&p));
        } else {
            out.push_str(&String::from_utf8_lossy(&fs::read(&p).unwrap()));
        }
    }
    out
}
