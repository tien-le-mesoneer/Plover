//! JUnit XML in the layout of Bruno's reporter (bruno-cli/src/reporters/junit.js):
//! one `<testsuite>` per request, one `<testcase>` per assert and test.

use std::fmt::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{POST_RESPONSE_SCRIPT_ERROR, RecordStatus, RequestRecord};

pub fn render(records: &[RequestRecord]) -> String {
    let timestamp = iso_timestamp(SystemTime::now());
    let hostname = std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "localhost".to_owned());
    let mut out = String::from("<?xml version=\"1.0\"?>\n<testsuites>\n");
    for r in records {
        let classname = r.file.strip_suffix(".bru").unwrap_or(&r.file);
        let run_duration = r.duration().as_secs_f64();
        let mut cases = Vec::new();
        let mut failures = 0;
        for a in &r.outcome.asserts {
            cases.push((format!("{} {}", a.lhs, a.rhs), a.passed, a.error.clone()));
        }
        for t in &r.outcome.tests {
            cases.push((t.name.clone(), t.passed, t.error.clone()));
        }
        if let Some(message) = r.post_response_script_error() {
            cases.push((POST_RESPONSE_SCRIPT_ERROR.to_owned(), false, Some(message)));
        }
        let total = cases.len();
        let per_case = if total > 0 {
            run_duration / total as f64
        } else {
            0.0
        };
        let mut body = String::new();
        let (errors, tests, skipped) = match r.status {
            RecordStatus::Skipped => (0, total, 1),
            RecordStatus::Error => (1, 1, 0),
            _ => (0, total, 0),
        };
        if r.status == RecordStatus::Error {
            let _ = write!(
                body,
                "    <testcase name=\"Test suite has no errors\" status=\"fail\" classname=\"{}\" time=\"{:.3}\">\n      <error type=\"error\" message=\"{}\"/>\n    </testcase>\n",
                escape(classname),
                run_duration,
                escape(r.error.as_deref().unwrap_or_default())
            );
        } else {
            for (name, passed, error) in &cases {
                let status = if *passed { "pass" } else { "fail" };
                let _ = write!(
                    body,
                    "    <testcase name=\"{}\" status=\"{status}\" classname=\"{}\" time=\"{per_case:.3}\"",
                    escape(name),
                    escape(classname)
                );
                if *passed {
                    body.push_str("/>\n");
                } else {
                    failures += 1;
                    let _ = write!(
                        body,
                        ">\n      <failure type=\"failure\" message=\"{}\"/>\n    </testcase>\n",
                        escape(error.as_deref().unwrap_or_default())
                    );
                }
            }
        }
        if r.status == RecordStatus::Skipped {
            body.push_str("    <skipped message=\"Request Skipped\"/>\n");
        }
        let _ = write!(
            out,
            "  <testsuite name=\"{}\" file=\"{}\" errors=\"{errors}\" failures=\"{failures}\" skipped=\"{skipped}\" tests=\"{tests}\" timestamp=\"{timestamp}\" hostname=\"{}\" time=\"{run_duration:.3}\"",
            escape(&r.name),
            escape(&r.file),
            escape(&hostname)
        );
        if body.is_empty() {
            out.push_str("/>\n");
        } else {
            out.push_str(">\n");
            out.push_str(&body);
            out.push_str("  </testsuite>\n");
        }
    }
    out.push_str("</testsuites>\n");
    out
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// `new Date().toISOString().split('Z')[0]`: UTC, milliseconds, no `Z`.
pub(super) fn iso_timestamp(now: SystemTime) -> String {
    let since = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's civil_from_days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        since.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamp_format() {
        let t = UNIX_EPOCH + Duration::from_millis(1_761_134_400_123);
        assert_eq!(iso_timestamp(t), "2025-10-22T12:00:00.123");
    }

    #[test]
    fn escapes_attributes() {
        assert_eq!(
            escape("a<b & \"c\" 'd'\n"),
            "a&lt;b &amp; &quot;c&quot; &apos;d&apos;&#10;"
        );
    }
}
