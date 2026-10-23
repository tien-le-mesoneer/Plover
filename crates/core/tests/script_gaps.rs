//! Script gaps A–D: process.env/.env, extra safe-mode modules, sendRequest/axios/cookies,
//! persisted variable changes (docs/superpowers/plans/2026-10-23-script-gaps.md).

use std::fs;
use std::path::Path;

use brs_core::collection::Collection;
use brs_core::runner::{RunEvent, RunOptions, RunReport, run};
use wiremock::matchers::{header, path};
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
