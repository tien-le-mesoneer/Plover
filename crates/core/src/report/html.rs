//! A self-contained HTML report (`--reporter-html`): inline CSS, no scripts, no external
//! resources, so it opens offline and from CI artifacts. Loosely follows Bruno's report:
//! a summary, then one row per request with its failing checks and a collapsible
//! `<details>` with the request and response.

use std::fmt::Write;
use std::time::{Duration, UNIX_EPOCH};

use super::ReportFilter;
use crate::run_record::{
    BodyDetail, POST_RESPONSE_SCRIPT_ERROR, PartDetail, ResultRecord, ResultStatus, RunRecord,
};

const STYLE: &str = "body{font:14px/1.5 system-ui,sans-serif;margin:24px;color:#1b1d22;background:#fff}\
h1{font-size:20px;margin:0 0 4px}.meta{color:#555;margin:0 0 16px}\
ul.summary{list-style:none;padding:0;display:flex;flex-wrap:wrap;gap:8px 20px;margin:0 0 20px}\
table{border-collapse:collapse;width:100%}th,td{text-align:left;padding:6px 8px;border-bottom:1px solid #d0d4dc;vertical-align:top}\
th{font-weight:600;background:#f3f4f7}.pass{color:#16672f}.fail,.error{color:#a3201b}.skipped{color:#5b5f6b}\
.checks{margin:4px 0 0;padding-left:18px}.checks li{margin:2px 0}code{font-family:ui-monospace,monospace;font-size:13px}\
details{margin:0 0 4px}summary{cursor:pointer;color:#3a3f4b}h3{font-size:14px;margin:10px 0 4px}\
pre{white-space:pre-wrap;overflow-wrap:anywhere;background:#f3f4f7;padding:8px;margin:4px 0;max-height:480px;overflow:auto}\
table.kv td{border:none;padding:1px 8px 1px 0}.note{color:#5b5f6b}\
@media (prefers-color-scheme:dark){summary{color:#c3c7d0}pre{background:#262931}.note{color:#a9adb7}body{background:#1b1d22;color:#e6e7ea}.meta{color:#a9adb7}th{background:#262931}\
th,td{border-color:#3a3e48}.pass{color:#6ee09a}.fail,.error{color:#ff8a80}.skipped{color:#a9adb7}}";

pub fn render(record: &RunRecord, filter: &ReportFilter) -> String {
    let report = &record.report;
    let records = &record.results;
    let mut out =
        String::from("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<title>Plover run report</title>\n<style>");
    out.push_str(STYLE);
    out.push_str("</style>\n</head>\n<body>\n<h1>Plover run report</h1>\n");
    let env = record
        .header
        .env
        .as_deref()
        .map(|e| format!(" · {}", escape(e)))
        .unwrap_or_default();
    let _ = writeln!(
        out,
        "<p class=\"meta\">{}{env} · {} UTC · {:.2} s</p>",
        escape(&record.header.collection_name),
        crate::time::iso_utc(UNIX_EPOCH + Duration::from_millis(record.header.started_at_ms)),
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
    let sections = super::iterations(record);
    let rows = |index: usize| {
        record
            .iterations
            .iter()
            .find(|i| i.index == index)
            .map(|i| &i.data)
    };
    let grouped = sections.len() > 1 || rows(0).is_some_and(|d| !d.is_empty());
    if grouped {
        let total = record.report.iterations.max(sections.len());
        for (index, results) in &sections {
            let _ = writeln!(
                out,
                "<section>\n<h2>Iteration {} of {total}</h2>",
                index + 1
            );
            if let Some(data) = rows(*index).filter(|d| !d.is_empty()) {
                out.push_str("<table class=\"kv\">");
                for (k, v) in data {
                    let v = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    let _ = write!(
                        out,
                        "<tr><td><code>{}</code></td><td><code>{}</code></td></tr>",
                        escape(k),
                        escape(&v)
                    );
                }
                out.push_str("</table>\n");
            }
            let count = |st: ResultStatus| results.iter().filter(|r| r.status == st).count();
            let _ = writeln!(
                out,
                "<p class=\"meta\">{} passed · {} failed · {} skipped</p>",
                count(ResultStatus::Pass),
                count(ResultStatus::Fail) + count(ResultStatus::Error),
                count(ResultStatus::Skipped)
            );
            results_table(&mut out, results, filter);
            out.push_str("</section>\n");
        }
    } else {
        let all: Vec<&ResultRecord> = records.iter().collect();
        results_table(&mut out, &all, filter);
    }
    out.push_str("</body>\n</html>\n");
    out
}

fn results_table(out: &mut String, records: &[&ResultRecord], filter: &ReportFilter) {
    out.push_str(
        "<table>\n<thead><tr><th>Result</th><th>Method</th><th>Name</th><th>File</th>\
<th>Status</th><th>Time</th><th>Checks</th></tr></thead>\n<tbody>\n",
    );
    for r in records {
        let (class, label) = match r.status {
            ResultStatus::Pass => ("pass", "✓ Passed"),
            ResultStatus::Fail => ("fail", "✗ Failed"),
            ResultStatus::Error => ("error", "✗ Error"),
            ResultStatus::Skipped => ("skipped", "– Skipped"),
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
        if r.status == ResultStatus::Error
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
        if r.outcome.request.is_some() {
            out.push_str("<tr class=\"detail\"><td colspan=\"7\">");
            detail(out, r, filter);
            out.push_str("</td></tr>\n");
        }
    }
    out.push_str("</tbody>\n</table>\n");
}

/// The request and response of one result, collapsed by default.
fn detail(out: &mut String, r: &ResultRecord, filter: &ReportFilter) {
    out.push_str("<details><summary>Request and response</summary>");
    if let Some(req) = &r.outcome.request {
        let _ = write!(
            out,
            "<h3>Request</h3><p><code>{} {}</code></p>",
            escape(&req.method),
            escape(&req.url)
        );
        headers(out, &req.headers, filter);
        if !filter.skip_request_body {
            body(out, &req.body);
        }
    }
    match &r.outcome.response {
        Some(res) => {
            let _ = write!(
                out,
                "<h3>Response</h3><p><code>{} {}</code></p>",
                res.status,
                escape(&res.status_text)
            );
            headers(out, &res.headers, filter);
            if !filter.skip_response_body {
                body(out, &res.body);
            }
        }
        None => out.push_str("<h3>Response</h3><p class=\"note\">No response</p>"),
    }
    out.push_str("</details>");
}

fn headers(out: &mut String, headers: &[(String, String)], filter: &ReportFilter) {
    let shown = filter.headers(headers);
    if shown.is_empty() {
        return;
    }
    out.push_str("<table class=\"kv\">");
    for (name, value) in shown {
        let _ = write!(
            out,
            "<tr><td><code>{}</code></td><td><code>{}</code></td></tr>",
            escape(name),
            escape(value)
        );
    }
    out.push_str("</table>");
}

fn body(out: &mut String, body: &BodyDetail) {
    match body {
        BodyDetail::None => {}
        BodyDetail::Text {
            text,
            size,
            truncated,
        } => {
            let _ = write!(out, "<pre>{}</pre>", escape(text));
            if *truncated {
                let _ = write!(
                    out,
                    "<p class=\"note\">Showing the first {} of {size} bytes</p>",
                    text.len()
                );
            }
        }
        BodyDetail::Base64 { size, .. } => {
            let _ = write!(out, "<p class=\"note\">Binary body, {size} bytes</p>");
        }
        BodyDetail::Form { fields } => {
            out.push_str("<table class=\"kv\">");
            for (k, v) in fields {
                let _ = write!(
                    out,
                    "<tr><td><code>{}</code></td><td><code>{}</code></td></tr>",
                    escape(k),
                    escape(v)
                );
            }
            out.push_str("</table>");
        }
        BodyDetail::Multipart { parts } => {
            out.push_str("<table class=\"kv\">");
            for p in parts {
                let (name, value) = match p {
                    PartDetail::Text { name, value, .. } => (name, escape(value)),
                    PartDetail::File {
                        name, file_name, ..
                    } => (name, format!("file {}", escape(file_name))),
                };
                let _ = write!(
                    out,
                    "<tr><td><code>{}</code></td><td><code>{value}</code></td></tr>",
                    escape(name)
                );
            }
            out.push_str("</table>");
        }
        BodyDetail::Omitted { reason, size } => {
            let _ = write!(
                out,
                "<p class=\"note\">Body not kept ({}), {size} bytes</p>",
                escape(reason)
            );
        }
    }
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
