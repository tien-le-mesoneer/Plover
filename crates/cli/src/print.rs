//! Human-readable output for run events.

use std::path::{Path, PathBuf};

use brs_core::runner::{RunEvent, RunReport};

pub struct Printer {
    root: PathBuf,
    current: Option<(String, String)>,
}

impl Printer {
    pub fn new(root: &Path) -> Printer {
        Printer {
            root: root.to_path_buf(),
            current: None,
        }
    }

    pub fn event(&mut self, event: &RunEvent) {
        match event {
            RunEvent::Started { name, method, .. } => {
                self.current = Some((method.clone(), name.clone()))
            }
            RunEvent::Passed {
                path,
                status,
                elapsed,
                size,
            } => {
                let (method, name) = self
                    .current
                    .take()
                    .unwrap_or_else(|| (String::new(), self.relative(path)));
                println!(
                    "✓ {method:<7} {name}  {status}  {}ms  {}",
                    elapsed.as_millis(),
                    format_size(*size)
                );
            }
            RunEvent::Failed { path, error } => {
                self.current = None;
                println!("✗ {}: {error}", self.relative(path));
            }
            RunEvent::Skipped { path, reason } => println!("- {}: {reason}", self.relative(path)),
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
        "{} passed, {} failed, {} skipped ({}ms)",
        report.passed,
        report.failed,
        report.skipped,
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
