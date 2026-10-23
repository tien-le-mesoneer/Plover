//! A self-contained HTML report (`--reporter-html`): inline CSS, no scripts, no external
//! resources, so it opens offline and from CI artifacts. Loosely follows Bruno's report:
//! a summary, then one row per request with its failing checks.

use std::fmt::Write;
use std::time::SystemTime;

use plover_core::runner::RunReport;

use super::{POST_RESPONSE_SCRIPT_ERROR, RecordStatus, RequestRecord};

const STYLE: &str = "body{font:14px/1.5 system-ui,sans-serif;margin:24px;color:#1b1d22;background:#fff}\
h1{font-size:20px;margin:0 0 4px}.meta{color:#555;margin:0 0 16px}\
ul.summary{list-style:none;padding:0;display:flex;flex-wrap:wrap;gap:8px 20px;margin:0 0 20px}\
table{border-collapse:collapse;width:100%}th,td{text-align:left;padding:6px 8px;border-bottom:1px solid #d0d4dc;vertical-align:top}\
th{font-weight:600;background:#f3f4f7}.pass{color:#16672f}.fail,.error{color:#a3201b}.skipped{color:#5b5f6b}\
.checks{margin:4px 0 0;padding-left:18px}.checks li{margin:2px 0}code{font-family:ui-monospace,monospace;font-size:13px}\
@media (prefers-color-scheme:dark){body{background:#1b1d22;color:#e6e7ea}.meta{color:#a9adb7}th{background:#262931}\
th,td{border-color:#3a3e48}.pass{color:#6ee09a}.fail,.error{color:#ff8a80}.skipped{color:#a9adb7}}";

pub fn render(records: &[RequestRecord], report: &RunReport) -> String {
    let mut out =
        String::from("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<title>Plover run report</title>\n<style>");
    out.push_str(STYLE);
    out.push_str("</style>\n</head>\n<body>\n<h1>Plover run report</h1>\n");
    let _ = writeln!(
        out,
        "<p class=\"meta\">{} UTC · {:.2} s</p>",
        super::junit::iso_timestamp(SystemTime::now()),
        report.elapsed.as_secs_f64()
    );
    let tests = report.tests_passed + report.tests_failed;
    let asserts = report.asserts_passed + report.asserts_failed;
    let _ = writeln!(
        out,
        "<ul class=\"summary\"><li><strong>Requests</strong>: {} total</li><li class=\"pass\">{} passed</li>\
<li class=\"fail\">{} failed</li><li class=\"skipped\">{} skipped</li>\
<li><strong>Tests</strong>: {}/{}</li><li><strong>Asserts</strong>: {}/{}</li></ul>",
        report.passed + report.failed + report.skipped,
        report.passed,
        report.failed,
        report.skipped,
        report.tests_passed,
        tests,
        report.asserts_passed,
        asserts
    );
    out.push_str(
        "<table>\n<thead><tr><th>Result</th><th>Method</th><th>Name</th><th>File</th>\
<th>Status</th><th>Time</th><th>Checks</th></tr></thead>\n<tbody>\n",
    );
    for r in records {
        let (class, label) = match r.status {
            RecordStatus::Pass => ("pass", "✓ Passed"),
            RecordStatus::Fail => ("fail", "✗ Failed"),
            RecordStatus::Error => ("error", "✗ Error"),
            RecordStatus::Skipped => ("skipped", "– Skipped"),
        };
        let status = r.outcome.status.map(|s| s.to_string()).unwrap_or_default();
        let checks_total = r.outcome.tests.len() + r.outcome.asserts.len();
        let checks_passed = r.outcome.tests.iter().filter(|t| t.passed).count()
            + r.outcome.asserts.iter().filter(|a| a.passed).count();
        let _ = write!(
            out,
            "<tr class=\"{class}\"><td>{label}</td><td>{}</td><td>{}</td><td><code>{}</code></td>\
<td>{status}</td><td>{:.0} ms</td><td>{checks_passed}/{checks_total}",
            escape(&r.method),
            escape(&r.name),
            escape(&r.file),
            r.duration().as_secs_f64() * 1000.0
        );
        let mut problems: Vec<String> = Vec::new();
        if let Some(reason) = &r.skip_reason {
            problems.push(escape(reason));
        }
        if r.status == RecordStatus::Error
            && let Some(e) = &r.error
        {
            problems.push(escape(e));
        }
        for a in r.outcome.asserts.iter().filter(|a| !a.passed) {
            problems.push(format!(
                "<code>{} {}</code>: {}",
                escape(&a.lhs),
                escape(&a.rhs),
                escape(a.error.as_deref().unwrap_or_default())
            ));
        }
        for t in r.outcome.tests.iter().filter(|t| !t.passed) {
            problems.push(format!(
                "{}: {}",
                escape(&t.name),
                escape(t.error.as_deref().unwrap_or_default())
            ));
        }
        if let Some(message) = r.post_response_script_error() {
            problems.push(format!(
                "{POST_RESPONSE_SCRIPT_ERROR}: {}",
                escape(&message)
            ));
        }
        if !problems.is_empty() {
            out.push_str("<ul class=\"checks\">");
            for p in problems {
                let _ = write!(out, "<li>{p}</li>");
            }
            out.push_str("</ul>");
        }
        out.push_str("</td></tr>\n");
    }
    out.push_str("</tbody>\n</table>\n</body>\n</html>\n");
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
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::escape;

    #[test]
    fn escapes_markup() {
        assert_eq!(
            escape("<script>a&'\"</script>"),
            "&lt;script&gt;a&amp;&#39;&quot;&lt;/script&gt;"
        );
    }
}
