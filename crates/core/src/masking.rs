//! Masking of a run before it is stored or reported, in two passes:
//!
//! - **By name:** credential-named values in bodies (`credentials::body_credential_spans`),
//!   form fields, multipart text parts and URL query values are masked whole, whatever
//!   their length or type.
//! - **By value:** every secret value, every credential sent or received in a credential
//!   or token header, every credential found by name (4 characters or more) and every JWT
//!   is replaced with `***` wherever it appears in the run (a token a login response
//!   returned is masked in that body too, because a later request sends it).

use crate::credentials::{
    SpanKind, body_credential_spans, is_credential_field, is_token_header, is_token_query_param,
    jwt_spans,
};
use crate::run_record::{BodyDetail, IterationRecord, PartDetail, ResultRecord, RunRecord};
use crate::script::ScriptError;
use crate::vars::ValueMap;

pub const MASK: &str = "***";
/// Headers whose values are always masked whole, and whose values are masked elsewhere
/// (as are those of `credentials::TOKEN_HEADERS`).
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
            collect_result(r, &mut values);
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

    /// By name: credential spans of a body's text are masked whole, back to front. A JSON
    /// span becomes the string `"***"` so the JSON stays valid.
    fn mask_named(&self, text: &mut String) {
        for span in body_credential_spans(text).into_iter().rev() {
            let masked = match span.kind {
                SpanKind::JsonNumber => "\"***\"",
                _ => MASK,
            };
            text.replace_range(span.range, masked);
        }
    }

    /// By name: values of credential and token query parameters.
    fn mask_query(&self, url: &mut String) {
        let masked = query_with(url, |name, value| {
            if is_named_query(name) && !value.is_empty() {
                MASK.to_owned()
            } else {
                value.to_owned()
            }
        });
        if let Some(masked) = masked {
            *url = masked;
        }
    }

    fn mask_url(&self, url: &mut String) {
        self.mask_query(url);
        self.mask(url);
    }

    fn mask_body(&self, body: &mut BodyDetail) {
        match body {
            BodyDetail::Text { text, .. } => {
                self.mask_named(text);
                self.mask(text);
            }
            BodyDetail::Form { fields } => {
                for (name, value) in fields {
                    if is_credential_field(name) && !value.is_empty() {
                        *value = MASK.to_owned();
                    } else {
                        self.mask(value);
                    }
                }
            }
            BodyDetail::Multipart { parts } => {
                for p in parts {
                    if let PartDetail::Text { name, value, .. } = p {
                        if is_credential_field(name) && !value.is_empty() {
                            *value = MASK.to_owned();
                        } else {
                            self.mask(value);
                        }
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
        self.mask_url(&mut result.url);
        for text in [&mut result.error, &mut result.skip_reason]
            .into_iter()
            .flatten()
        {
            self.mask(text);
        }
        let o = &mut result.outcome;
        if let Some(request) = o.request.as_mut() {
            self.mask_url(&mut request.url);
            self.mask_headers(&mut request.headers);
            self.mask_body(&mut request.body);
        }
        if let Some(response) = o.response.as_mut() {
            self.mask_url(&mut response.url);
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
            self.mask_url(&mut s.url);
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

    /// The row: credential-named keys at any depth become `"***"` whatever their type
    /// or length (a numeric or short data-file credential is masked by name only), then
    /// the other string values are searched for the collected values.
    pub fn mask_iteration(&self, iteration: &mut IterationRecord) {
        for (key, value) in iteration.data.iter_mut() {
            if is_credential_field(key) {
                *value = serde_json::Value::String(MASK.to_owned());
            } else {
                self.mask_value(value);
            }
        }
    }

    fn mask_value(&self, value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(s) => self.mask(s),
            serde_json::Value::Array(items) => items.iter_mut().for_each(|v| self.mask_value(v)),
            serde_json::Value::Object(map) => {
                for (key, v) in map.iter_mut() {
                    if is_credential_field(key) {
                        *v = serde_json::Value::String(MASK.to_owned());
                    } else {
                        self.mask_value(v);
                    }
                }
            }
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
    MASKED_HEADERS.iter().any(|h| name.eq_ignore_ascii_case(h)) || is_token_header(name)
}

fn is_named_query(name: &str) -> bool {
    is_credential_field(name) || is_token_query_param(name)
}

/// `url` with each query value replaced by `f(name, value)`, or `None` without a query.
fn query_with(url: &str, mut f: impl FnMut(&str, &str) -> String) -> Option<String> {
    let (base, rest) = url.split_once('?')?;
    let (query, fragment) = match rest.split_once('#') {
        Some((q, frag)) => (q, Some(frag)),
        None => (rest, None),
    };
    let pairs: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) => format!("{name}={}", f(name, value)),
            None => pair.to_owned(),
        })
        .collect();
    let mut out = format!("{base}?{}", pairs.join("&"));
    if let Some(frag) = fragment {
        out.push('#');
        out.push_str(frag);
    }
    Some(out)
}

/// Pass 1 for one result: credential header values, credentials found by name (4
/// characters or more, strings only: a number is masked by name, never by value), and
/// every JWT.
fn collect_result(r: &ResultRecord, out: &mut Vec<String>) {
    let o = &r.outcome;
    let request = o.request.iter().flat_map(|d| &d.headers);
    let response = o.response.iter().flat_map(|d| &d.headers);
    for (name, value) in request.chain(response) {
        collect_header(name, value, out);
        collect_jwts(value, out);
    }
    let mut named = |v: &str| {
        if v.len() >= MIN_SECRET_VALUE {
            out.push(v.to_owned());
        }
    };
    let urls = std::iter::once(&r.url)
        .chain(o.request.iter().map(|d| &d.url))
        .chain(o.response.iter().map(|d| &d.url));
    for url in urls {
        query_with(url, |name, value| {
            if is_named_query(name) {
                named(value);
            }
            String::new()
        });
    }
    for body in o
        .request
        .iter()
        .map(|d| &d.body)
        .chain(o.response.iter().map(|d| &d.body))
    {
        match body {
            BodyDetail::Text { text, .. } => {
                for span in body_credential_spans(text) {
                    if span.kind != SpanKind::JsonNumber {
                        named(&text[span.range]);
                    }
                }
            }
            BodyDetail::Form { fields } => fields
                .iter()
                .filter(|(n, _)| is_credential_field(n))
                .for_each(|(_, v)| named(v)),
            BodyDetail::Multipart { parts } => {
                for p in parts {
                    if let PartDetail::Text { name, value, .. } = p
                        && is_credential_field(name)
                    {
                        named(value);
                    }
                }
            }
            BodyDetail::None | BodyDetail::Base64 { .. } | BodyDetail::Omitted { .. } => {}
        }
    }
    let texts = std::iter::once(&r.url)
        .chain(o.request.iter().map(|d| &d.url))
        .chain(o.response.iter().map(|d| &d.url))
        .chain(o.console.iter().map(|l| &l.text))
        .chain(o.scripted_requests.iter().map(|s| &s.url));
    for t in texts {
        collect_jwts(t, out);
    }
}

fn collect_jwts(text: &str, out: &mut Vec<String>) {
    out.extend(jwt_spans(text).into_iter().map(|r| text[r].to_owned()));
}

/// Credential values sent in `headers` (whole `Authorization`, `Proxy-Authorization`,
/// `Cookie` and token header values, the part after the scheme, each cookie value), 8
/// characters or more; the same rule run records are masked by.
pub fn credential_values(headers: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, value) in headers {
        collect_header(name, value, &mut out);
    }
    out
}

/// The whole value of a credential header and its credential parts: the token after the
/// scheme (`Bearer x`), each cookie value (not `Set-Cookie` attributes).
fn collect_header(name: &str, value: &str, out: &mut Vec<String>) {
    let name = name.to_ascii_lowercase();
    if !is_masked_header(&name) {
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
        "set-cookie" => {
            let first = value.split(';').next().unwrap_or_default();
            if let Some((_, v)) = first.split_once('=') {
                push(v);
            }
        }
        // Token headers: the whole value only.
        _ => {}
    }
}
