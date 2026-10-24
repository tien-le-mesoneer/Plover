//! Compares a data-driven `plv run` with Bruno CLI on `fixtures/data-driven`.
//! Skipped unless `BRU_BIN` names a `bru` executable (Bruno CLI 4.3.0), e.g.
//! `BRU_BIN=/path/to/node_modules/.bin/bru cargo test -p plover-cli --test bruno_compat`.
//!
//! What must match: one report entry per row with its `iterationIndex`, the requests of
//! each iteration in order with their interpolated URLs, statuses, test and assert
//! results, and the per-iteration summary counts both reporters have. Known differences
//! that are not about iterations are ignored: Bruno's transport headers, `uid`s, extra
//! summary keys, and Plover masking the `password` column (a deliberate deviation).

use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;
use wiremock::matchers::path_regex;
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/data-driven");

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The parts of a JSON report a data-driven run must agree on.
fn essentials(report: &Value) -> Value {
    let entries = report.as_array().expect("report is a list of iterations");
    Value::Array(
        entries
            .iter()
            .map(|e| {
                let summary = &e["summary"];
                let keys = [
                    "totalRequests",
                    "passedRequests",
                    "failedRequests",
                    "errorRequests",
                    "skippedRequests",
                    "totalAssertions",
                    "passedAssertions",
                    "failedAssertions",
                    "totalTests",
                    "passedTests",
                    "failedTests",
                ];
                let results: Vec<Value> = e["results"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| {
                        // Bruno shows the password; Plover masks the credential column.
                        let data = r["request"]["data"]
                            .as_str()
                            .map(|d| {
                                let mut d = d.to_owned();
                                for n in 1..=9 {
                                    d = d.replace(&format!("pw-fake-000{n}"), "***");
                                }
                                Value::String(d)
                            })
                            .unwrap_or(Value::Null);
                        serde_json::json!({
                            "iterationIndex": r["iterationIndex"],
                            "name": r["name"],
                            "method": r["request"]["method"],
                            "url": r["request"]["url"],
                            "data": data,
                            "status": r["response"]["status"],
                            "result": r["status"],
                            "asserts": r["assertionResults"].as_array().unwrap().iter().map(|a| serde_json::json!([a["lhsExpr"], a["rhsExpr"], a["status"]])).collect::<Vec<_>>(),
                            "tests": r["testResults"].as_array().unwrap().iter().map(|t| serde_json::json!([t["description"], t["status"]])).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                serde_json::json!({
                    "iterationIndex": e["iterationIndex"],
                    "summary": keys.iter().map(|k| (k.to_string(), summary[k].clone())).collect::<serde_json::Map<_, _>>(),
                    "results": results,
                })
            })
            .collect(),
    )
}

#[tokio::test]
async fn data_driven_json_matches_bruno() {
    let Some(bru) = std::env::var_os("BRU_BIN") else {
        eprintln!("BRU_BIN is not set; skipping the Bruno comparison");
        return;
    };
    let mock = MockServer::start().await;
    Mock::given(path_regex(".*"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"ok":true}"#))
        .mount(&mock)
        .await;
    let host = format!("host={}", mock.uri());
    for (flag, file) in [
        ("--csv-file-path", "rows.csv"),
        ("--json-file-path", "rows.json"),
    ] {
        let work = tempfile::tempdir().unwrap();
        let bru_dir = work.path().join("bru");
        let plv_dir = work.path().join("plv");
        copy_dir(Path::new(FIXTURE), &bru_dir);
        copy_dir(Path::new(FIXTURE), &plv_dir);
        let b = work.path().join("b.json");
        let p = work.path().join("p.json");
        let out = std::process::Command::new(&bru)
            .current_dir(&bru_dir)
            .args([
                "run",
                ".",
                "--env",
                "local",
                "--env-var",
                &host,
                flag,
                file,
                "--reporter-json",
            ])
            .arg(&b)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "bru: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        let state = tempfile::tempdir().unwrap();
        let out = Command::cargo_bin("plv")
            .unwrap()
            .env("PLOVER_STATE_DIR", state.path())
            .current_dir(&plv_dir)
            .args([
                "run",
                ".",
                "--env",
                "local",
                "--env-var",
                &host,
                flag,
                file,
                "--reporter-json",
            ])
            .arg(&p)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "plv: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        let read = |path: &Path| -> Value {
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
        };
        let (bruno, plover) = (essentials(&read(&b)), essentials(&read(&p)));
        assert_eq!(plover, bruno, "{flag}\nplv: {plover:#}\nbru: {bruno:#}");
    }
}
