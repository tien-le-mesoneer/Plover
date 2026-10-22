//! Human-readable output for run events.

use std::path::{Path, PathBuf};

use brs_core::runner::{RequestOutcome, RunEvent, RunReport, ScriptPhase};

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
            RunEvent::Skipped { path, reason } => println!("- {}: {reason}", self.relative(path)),
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

fn format_size(bytes: usize) -> String {
    match bytes {
        b if b < 1024 => format!("{b}B"),
        b if b < 1024 * 1024 => format!("{:.1}KB", b as f64 / 1024.0),
        b => format!("{:.1}MB", b as f64 / (1024.0 * 1024.0)),
    }
}
