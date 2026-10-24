//! Human-readable output for run events.

use std::path::{Path, PathBuf};

use std::time::{Duration, UNIX_EPOCH};

use plover_core::run_history::RunMeta;
use plover_core::run_record::{ResultStatus, RunRecord};
use plover_core::runner::{RequestOutcome, RunEvent, RunReport, ScriptPhase};

pub struct Printer {
    root: PathBuf,
    current: Option<(String, String)>,
    /// Also print script console output.
    verbose: bool,
}

impl Printer {
    pub fn new(root: &Path, verbose: bool) -> Printer {
        Printer {
            root: root.to_path_buf(),
            current: None,
            verbose,
        }
    }

    pub fn event(&mut self, event: &RunEvent) {
        match event {
            RunEvent::Started { name, method, .. } => {
                self.current = Some((method.clone(), name.clone()))
            }
            RunEvent::Passed { path, outcome } => {
                self.request_line('✓', path, outcome);
                self.details(outcome);
            }
            RunEvent::Failed {
                path,
                error,
                outcome,
                ..
            } => {
                if outcome.status.is_some() {
                    self.request_line('✗', path, outcome);
                    if let Some(f) = &outcome.script_error
                        && f.phase != ScriptPhase::Tests
                    {
                        println!("    ✗ {} script error: {}", f.phase, f.error);
                    }
                } else {
                    self.current = None;
                    println!("✗ {}: {error}", self.relative(path));
                }
                self.details(outcome);
            }
            RunEvent::Skipped { path, reason, .. } => {
                println!("- {}: {reason}", self.relative(path))
            }
            // The summary line is printed by `summary`.
            RunEvent::Finished { .. } => {}
        }
    }

    fn request_line(&mut self, mark: char, path: &Path, outcome: &RequestOutcome) {
        let (method, name) = self
            .current
            .take()
            .unwrap_or_else(|| (String::new(), self.relative(path)));
        println!(
            "{mark} {method:<7} {name}  {}  {}ms  {}",
            outcome.status.unwrap_or_default(),
            outcome.elapsed.as_millis(),
            format_size(outcome.size)
        );
    }

    fn details(&self, outcome: &RequestOutcome) {
        for a in &outcome.asserts {
            match &a.error {
                None if a.passed => println!("    ✓ assert {}: {}", a.lhs, a.rhs),
                error => println!(
                    "    ✗ assert {}: {}: {}",
                    a.lhs,
                    a.rhs,
                    error.as_deref().unwrap_or("failed")
                ),
            }
        }
        for t in &outcome.tests {
            if t.passed {
                println!("    ✓ {}", t.name);
            } else {
                println!(
                    "    ✗ {}: {}",
                    t.name,
                    t.error.as_deref().unwrap_or("failed")
                );
            }
        }
        if self.verbose {
            for line in &outcome.console {
                for text in line.text.lines() {
                    println!("    │ {text}");
                }
            }
            for r in &outcome.scripted_requests {
                let result = match (r.status, &r.error) {
                    (Some(status), _) => status.to_string(),
                    (None, Some(error)) => format!("error: {error}"),
                    (None, None) => "no response".to_owned(),
                };
                println!(
                    "    ↳ {} {} {} → {result} ({} ms)",
                    r.source, r.method, r.url, r.duration_ms
                );
            }
        }
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }
}

pub fn summary(report: &RunReport) {
    println!(
        "Requests: {} passed, {} failed, {} skipped · Tests: {}/{} · Asserts: {}/{} ({}ms)",
        report.passed,
        report.failed,
        report.skipped,
        report.tests_passed,
        report.tests_passed + report.tests_failed,
        report.asserts_passed,
        report.asserts_passed + report.asserts_failed,
        report.elapsed.as_millis()
    );
}

fn started(ms: u64) -> String {
    let iso = plover_core::time::iso_utc(UNIX_EPOCH + Duration::from_millis(ms));
    // `2025-10-22T12:00:00.123` → `2025-10-22 12:00:00`
    format!(
        "{} UTC",
        iso.replace('T', " ").split('.').next().unwrap_or_default()
    )
}

/// `plv runs`: newest first.
pub fn run_list(metas: &[RunMeta]) {
    println!(
        "{:<23}  {:<23}  {:<6} {:<12} {:>17}  TIME",
        "ID", "STARTED", "SOURCE", "ENV", "PASSED/FAILED/SKIP"
    );
    for m in metas {
        let r = &m.report;
        let counts = format!("{}/{}/{}", r.passed, r.failed, r.skipped);
        let cancelled = if m.outcome.as_str() == "cancelled" {
            "  cancelled"
        } else {
            ""
        };
        println!(
            "{:<23}  {:<23}  {:<6} {:<12} {:>17}  {}ms{cancelled}",
            m.header.id,
            started(m.header.started_at_ms),
            m.header.source.as_str(),
            m.header.env.as_deref().unwrap_or("-"),
            counts,
            r.elapsed.as_millis()
        );
    }
}

/// `plv runs show`: the run as `plv run` summarised it (values masked).
pub fn run_record(record: &RunRecord) {
    let h = &record.header;
    println!(
        "Run {} · {} · {} · {} · {} · {}",
        h.id,
        h.collection_name,
        h.target,
        h.env.as_deref().unwrap_or("no environment"),
        started(h.started_at_ms),
        h.source.as_str()
    );
    for r in &record.results {
        let o = &r.outcome;
        match r.status {
            ResultStatus::Skipped => println!(
                "- {}: {}",
                r.file,
                r.skip_reason.as_deref().unwrap_or("skipped")
            ),
            ResultStatus::Error => {
                println!("✗ {}: {}", r.file, r.error.as_deref().unwrap_or("error"))
            }
            status => {
                let mark = if status == ResultStatus::Pass {
                    '✓'
                } else {
                    '✗'
                };
                println!(
                    "{mark} {:<7} {}  {}  {}ms  {}",
                    r.method,
                    r.name,
                    o.status.unwrap_or_default(),
                    o.elapsed.as_millis(),
                    format_size(o.size)
                );
                for a in o.asserts.iter().filter(|a| !a.passed) {
                    println!(
                        "    ✗ assert {}: {}: {}",
                        a.lhs,
                        a.rhs,
                        a.error.as_deref().unwrap_or("failed")
                    );
                }
                for t in o.tests.iter().filter(|t| !t.passed) {
                    println!(
                        "    ✗ {}: {}",
                        t.name,
                        t.error.as_deref().unwrap_or("failed")
                    );
                }
            }
        }
    }
    if record.outcome.as_str() == "cancelled" {
        println!("Run was cancelled");
    }
    summary(&record.report);
}

fn format_size(bytes: usize) -> String {
    match bytes {
        b if b < 1024 => format!("{b}B"),
        b if b < 1024 * 1024 => format!("{:.1}KB", b as f64 / 1024.0),
        b => format!("{:.1}MB", b as f64 / (1024.0 * 1024.0)),
    }
}
