//! Value-based masking of a run before it is stored or reported: every secret value and
//! every credential sent or received in a credential header is replaced with `***`,
//! wherever it appears in the run (a token a login response returned is masked in that
//! body too, because a later request sends it as `Authorization`).

use crate::run_record::{BodyDetail, IterationRecord, PartDetail, ResultRecord, RunRecord};
use crate::script::ScriptError;
use crate::vars::ValueMap;

pub const MASK: &str = "***";
/// Headers whose values are always masked whole, and whose values are masked elsewhere.
pub const MASKED_HEADERS: [&str; 4] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
];
/// Shorter header-derived values are not collected, so `***` does not spread over short
/// common strings.
const MIN_HEADER_VALUE: usize = 8;
/// Shorter secrets are not collected; they were marked secret on purpose, so the floor is
/// lower.
const MIN_SECRET_VALUE: usize = 4;

pub struct Masker {
    /// Longest first, so a value that contains another is masked whole.
    values: Vec<String>,
}

impl Masker {
    pub fn new(secrets: &[String], results: &[ResultRecord]) -> Masker {
        let mut values: Vec<String> = secrets
            .iter()
            .filter(|s| s.len() >= MIN_SECRET_VALUE)
            .cloned()
            .collect();
        for r in results {
            let request = r.outcome.request.iter().flat_map(|d| &d.headers);
            let response = r.outcome.response.iter().flat_map(|d| &d.headers);
            for (name, value) in request.chain(response) {
                collect_header(name, value, &mut values);
            }
        }
        values.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        values.dedup();
        Masker { values }
    }

    pub fn mask_text(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for v in &self.values {
            if out.contains(v.as_str()) {
                out = out.replace(v.as_str(), MASK);
            }
        }
        out
    }

    fn mask(&self, text: &mut String) {
        if self.values.iter().any(|v| text.contains(v.as_str())) {
            *text = self.mask_text(text);
        }
    }

    fn mask_headers(&self, headers: &mut [(String, String)]) {
        for (name, value) in headers {
            if is_masked_header(name) {
                *value = MASK.to_owned();
            } else {
                self.mask(value);
            }
        }
    }

    fn mask_body(&self, body: &mut BodyDetail) {
        match body {
            BodyDetail::Text { text, .. } => self.mask(text),
            BodyDetail::Form { fields } => fields.iter_mut().for_each(|(_, v)| self.mask(v)),
            BodyDetail::Multipart { parts } => {
                for p in parts {
                    if let PartDetail::Text { value, .. } = p {
                        self.mask(value);
                    }
                }
            }
            // Base64 bodies are not searched; the rest hold no values.
            BodyDetail::None | BodyDetail::Base64 { .. } | BodyDetail::Omitted { .. } => {}
        }
    }

    fn mask_script_error(&self, error: &mut ScriptError) {
        match error {
            ScriptError::Thrown { message, stack, .. } => {
                self.mask(message);
                if let Some(stack) = stack {
                    self.mask(stack);
                }
            }
            ScriptError::ForbiddenModule { message, .. } => self.mask(message),
            ScriptError::ResultUnavailable(message) | ScriptError::Internal(message) => {
                self.mask(message)
            }
            ScriptError::Timeout(_) | ScriptError::OutOfMemory | ScriptError::Unsettled => {}
        }
    }

    pub fn mask_result(&self, result: &mut ResultRecord) {
        if self.values.is_empty() && !has_masked_header(result) {
            return;
        }
        self.mask(&mut result.url);
        for text in [&mut result.error, &mut result.skip_reason]
            .into_iter()
            .flatten()
        {
            self.mask(text);
        }
        let o = &mut result.outcome;
        if let Some(request) = o.request.as_mut() {
            self.mask(&mut request.url);
            self.mask_headers(&mut request.headers);
            self.mask_body(&mut request.body);
        }
        if let Some(response) = o.response.as_mut() {
            self.mask(&mut response.url);
            self.mask_headers(&mut response.headers);
            self.mask_body(&mut response.body);
        }
        for line in &mut o.console {
            self.mask(&mut line.text);
        }
        for t in &mut o.tests {
            self.mask(&mut t.name);
            if let Some(e) = t.error.as_mut() {
                self.mask(e);
            }
        }
        for a in &mut o.asserts {
            self.mask(&mut a.lhs);
            self.mask(&mut a.rhs);
            if let Some(e) = a.error.as_mut() {
                self.mask(e);
            }
        }
        if let Some(f) = o.script_error.as_mut() {
            self.mask_script_error(&mut f.error);
        }
        for s in &mut o.scripted_requests {
            self.mask(&mut s.url);
            if let Some(e) = s.error.as_mut() {
                self.mask(e);
            }
        }
    }

    pub fn mask_record(&self, record: &mut RunRecord) {
        for r in &mut record.results {
            self.mask_result(r);
        }
        for i in &mut record.iterations {
            self.mask_iteration(i);
        }
    }

    /// String values of the row at any depth (data-file credentials are among the
    /// secrets the caller passed in).
    pub fn mask_iteration(&self, iteration: &mut IterationRecord) {
        for value in iteration.data.values_mut() {
            self.mask_value(value);
        }
    }

    fn mask_value(&self, value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(s) => self.mask(s),
            serde_json::Value::Array(items) => items.iter_mut().for_each(|v| self.mask_value(v)),
            serde_json::Value::Object(map) => map.values_mut().for_each(|v| self.mask_value(v)),
            _ => {}
        }
    }
}

/// Secret values as text: strings as-is, other JSON scalars via `to_string()`; objects and
/// arrays are skipped.
pub fn secret_strings(values: &ValueMap) -> Vec<String> {
    values
        .values()
        .filter_map(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(_) | serde_json::Value::Array(_) => None,
            serde_json::Value::Null => None,
            other => Some(other.to_string()),
        })
        .collect()
}

fn is_masked_header(name: &str) -> bool {
    MASKED_HEADERS.iter().any(|h| name.eq_ignore_ascii_case(h))
}

fn has_masked_header(r: &ResultRecord) -> bool {
    let request = r.outcome.request.iter().flat_map(|d| &d.headers);
    let response = r.outcome.response.iter().flat_map(|d| &d.headers);
    request
        .chain(response)
        .any(|(name, _)| is_masked_header(name))
}

/// The whole value of a credential header and its credential parts: the token after the
/// scheme (`Bearer x`), each cookie value (not `Set-Cookie` attributes).
fn collect_header(name: &str, value: &str, out: &mut Vec<String>) {
    let name = name.to_ascii_lowercase();
    if !MASKED_HEADERS.contains(&name.as_str()) {
        return;
    }
    let mut push = |v: &str| {
        let v = v.trim();
        if v.len() >= MIN_HEADER_VALUE {
            out.push(v.to_owned());
        }
    };
    push(value);
    match name.as_str() {
        "authorization" | "proxy-authorization" => {
            if let Some((_, credential)) = value.trim().split_once(' ') {
                push(credential);
            }
        }
        "cookie" => {
            for pair in value.split(';') {
                if let Some((_, v)) = pair.split_once('=') {
                    push(v);
                }
            }
        }
        _ => {
            let first = value.split(';').next().unwrap_or_default();
            if let Some((_, v)) = first.split_once('=') {
                push(v);
            }
        }
    }
}
