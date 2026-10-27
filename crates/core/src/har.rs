//! HAR files (HTTP Archive 1.2) to requests: a preview of the captured entries, and a
//! pure import plan that moves every credential into secret variables.
//!
//! Only the request side of an entry is imported. Response content is never read into
//! memory beyond the MIME type and size the preview shows.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use base64::Engine;
use serde::Deserialize;

use crate::credentials::{
    SpanKind, body_credential_spans, is_credential_field, is_token_header, is_token_query_param,
    looks_like_jwt,
};
use crate::curl::{body_from_text, query_from_url};
use crate::request::{Auth, Body, HttpRequest, KeyValue, Method, MultipartField, MultipartValue};
use crate::store::{sanitize_name, unique_name};

#[derive(Debug, Clone, Deserialize)]
pub struct Har {
    pub log: HarLog,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HarLog {
    #[serde(default)]
    pub pages: Vec<HarPage>,
    pub entries: Vec<HarEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HarPage {
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HarEntry {
    pub request: HarRequest,
    #[serde(default)]
    pub response: HarResponse,
    #[serde(default, rename = "_resourceType")]
    pub resource_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HarRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<HarNameValue>,
    #[serde(default, rename = "postData")]
    pub post_data: Option<HarPostData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HarNameValue {
    pub name: String,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HarPostData {
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub params: Vec<HarParam>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HarParam {
    pub name: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default, rename = "fileName")]
    pub file_name: Option<String>,
    #[serde(default, rename = "contentType")]
    pub content_type: Option<String>,
}

/// Status and content summary only; `content.text` is skipped while parsing.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HarResponse {
    #[serde(default)]
    pub status: Option<i64>,
    #[serde(default)]
    pub content: HarContent,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HarContent {
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum HarError {
    #[error("not a HAR file: {0}")]
    Invalid(String),
}

pub fn parse_har(text: &str) -> Result<Har, HarError> {
    serde_json::from_str(text).map_err(|e| HarError::Invalid(e.to_string()))
}

/// Why an entry is hidden (and unticked) in the import dialog by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenReason {
    Image,
    Script,
    Style,
    Font,
    Media,
    SourceMap,
    Preflight,
    DataUrl,
    Extension,
    WebSocket,
}

impl HiddenReason {
    pub fn as_str(self) -> &'static str {
        match self {
            HiddenReason::Image => "image",
            HiddenReason::Script => "script",
            HiddenReason::Style => "style",
            HiddenReason::Font => "font",
            HiddenReason::Media => "media",
            HiddenReason::SourceMap => "sourceMap",
            HiddenReason::Preflight => "preflight",
            HiddenReason::DataUrl => "dataUrl",
            HiddenReason::Extension => "extension",
            HiddenReason::WebSocket => "webSocket",
        }
    }
}

/// One row of the import dialog: an entry, or the first of a group of identical ones.
#[derive(Debug, Clone, PartialEq)]
pub struct HarRow {
    pub index: usize,
    pub method: String,
    pub url: String,
    pub host: String,
    pub status: Option<u16>,
    /// Response MIME type without parameters, lower-cased; `""` when unknown.
    pub mime: String,
    pub size: Option<u64>,
    /// The request carries a credential (moved to a secret on import) or a JWT-looking
    /// value anywhere (an XML or text body is imported as it is).
    pub has_secrets: bool,
    pub hidden: Option<HiddenReason>,
    /// How many entries have this method, URL and body (1 for a unique entry).
    pub duplicates: usize,
}

/// Rows in capture order; identical requests (method, URL and body) collapse into the
/// row of the first one.
pub fn preview(har: &Har) -> Vec<HarRow> {
    let mut rows: Vec<HarRow> = Vec::new();
    let mut seen: HashMap<(String, String, String), usize> = HashMap::new();
    for (index, entry) in har.log.entries.iter().enumerate() {
        let req = &entry.request;
        let key = (req.method.clone(), req.url.clone(), body_key(req));
        if let Some(&row) = seen.get(&key) {
            rows[row].duplicates += 1;
            continue;
        }
        seen.insert(key, rows.len());
        let content = &entry.response.content;
        rows.push(HarRow {
            index,
            method: req.method.clone(),
            url: req.url.clone(),
            host: host_and_path(&req.url).0,
            status: entry
                .response
                .status
                .filter(|s| *s > 0)
                .and_then(|s| u16::try_from(s).ok()),
            mime: bare_mime(content.mime_type.as_deref().unwrap_or_default()),
            size: content.size.and_then(|s| u64::try_from(s).ok()),
            has_secrets: has_secrets(req),
            hidden: hidden_reason(entry),
            duplicates: 1,
        });
    }
    rows
}

fn body_key(req: &HarRequest) -> String {
    match &req.post_data {
        None => String::new(),
        Some(pd) => match &pd.text {
            Some(text) => text.clone(),
            None => pd
                .params
                .iter()
                .map(|p| format!("{}={}", p.name, p.value.as_deref().unwrap_or_default()))
                .collect::<Vec<_>>()
                .join("&"),
        },
    }
}

fn bare_mime(mime: &str) -> String {
    mime.split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn hidden_reason(entry: &HarEntry) -> Option<HiddenReason> {
    let req = &entry.request;
    let url = req.url.to_ascii_lowercase();
    if url.starts_with("data:") {
        return Some(HiddenReason::DataUrl);
    }
    let scheme = url.split_once("://").map(|(s, _)| s).unwrap_or_default();
    if scheme.ends_with("-extension") {
        return Some(HiddenReason::Extension);
    }
    if scheme == "ws" || scheme == "wss" || entry.resource_type.as_deref() == Some("websocket") {
        return Some(HiddenReason::WebSocket);
    }
    if req.method.eq_ignore_ascii_case("OPTIONS") {
        return Some(HiddenReason::Preflight);
    }
    let path = host_and_path(&req.url).1.to_ascii_lowercase();
    let ext = path.rsplit_once('.').map(|(_, e)| e).unwrap_or_default();
    if ext == "map" {
        return Some(HiddenReason::SourceMap);
    }
    let mime = bare_mime(
        entry
            .response
            .content
            .mime_type
            .as_deref()
            .unwrap_or_default(),
    );
    mime_reason(&mime).or_else(|| extension_reason(ext))
}

fn mime_reason(mime: &str) -> Option<HiddenReason> {
    if mime.starts_with("image/") {
        Some(HiddenReason::Image)
    } else if mime.contains("javascript") || mime.contains("ecmascript") {
        Some(HiddenReason::Script)
    } else if mime == "text/css" {
        Some(HiddenReason::Style)
    } else if mime.starts_with("font/")
        || mime.contains("font-")
        || mime == "application/vnd.ms-fontobject"
    {
        Some(HiddenReason::Font)
    } else if mime.starts_with("audio/") || mime.starts_with("video/") {
        Some(HiddenReason::Media)
    } else {
        None
    }
}

fn extension_reason(ext: &str) -> Option<HiddenReason> {
    Some(match ext {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" | "avif" => {
            HiddenReason::Image
        }
        "js" | "mjs" | "cjs" => HiddenReason::Script,
        "css" => HiddenReason::Style,
        "woff" | "woff2" | "ttf" | "otf" | "eot" => HiddenReason::Font,
        "mp4" | "webm" | "mp3" | "wav" | "ogg" | "m4a" | "mov" => HiddenReason::Media,
        _ => return None,
    })
}

/// Host and path of a URL (no query or fragment); empty parts when it does not parse.
fn host_and_path(url: &str) -> (String, String) {
    if let Ok(parsed) = url::Url::parse(url) {
        return (
            parsed.host_str().unwrap_or_default().to_owned(),
            parsed.path().to_owned(),
        );
    }
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    match rest.split_once('/') {
        Some((host, path)) => (host.to_owned(), format!("/{path}")),
        None => (rest.to_owned(), String::new()),
    }
}

fn has_secrets(req: &HarRequest) -> bool {
    let header = req.headers.iter().any(|h| {
        let name = h.name.to_ascii_lowercase();
        !h.value.is_empty()
            && (name == "authorization"
                || name == "cookie"
                || is_token_header(&name)
                || looks_like_jwt(&h.value))
    });
    let query = query_from_url(&req.url).iter().any(|q| {
        !q.value.is_empty() && (is_token_query_param(&q.name) || looks_like_jwt(&q.value))
    });
    let body = req.post_data.as_ref().is_some_and(|pd| {
        pd.params.iter().any(|p| {
            is_credential_field(&p.name) && p.value.as_deref().is_some_and(|v| !v.is_empty())
        }) || pd.text.as_deref().is_some_and(|t| {
            // Numeric JSON credentials stay as written on import, so they do not flag.
            body_credential_spans(t)
                .iter()
                .any(|s| s.kind != SpanKind::JsonNumber)
                || crate::curl::decode_form(t)
                    .iter()
                    .any(|f| is_credential_field(&f.name) && !f.value.is_empty())
        })
    });
    header || query || body
}

// ---- import planning

/// The target environment's current secrets, so an import reuses a variable that holds
/// the same value and never overwrites one that holds another.
#[derive(Debug, Clone, Default)]
pub struct ExistingSecrets {
    /// Secret names with their current values.
    pub values: BTreeMap<String, String>,
    /// Names that may not be used: plain variables, and secrets whose value is unknown.
    pub plain_names: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportPlan {
    /// Display name and request, in capture order.
    pub requests: Vec<(String, HttpRequest)>,
    /// Secret variables to create, with their values.
    pub new_secrets: BTreeMap<String, String>,
    pub notices: Vec<String>,
}

/// Turns the chosen entries into requests. Pure: nothing is read or written.
///
/// - `existing_names`: lower-cased entry names in the target folder (`get x.bru`), so
///   the new request names do not collide with files there.
/// - `start_seq`: the highest seq already in the folder; requests get the next ones.
pub fn plan_import(
    har: &Har,
    entries: &[usize],
    existing_names: &BTreeSet<String>,
    start_seq: f64,
    secrets: &ExistingSecrets,
) -> ImportPlan {
    let mut indices: Vec<usize> = entries.to_vec();
    indices.sort_unstable();
    indices.dedup();
    let mut alloc = SecretAlloc {
        existing: secrets,
        new: BTreeMap::new(),
    };
    let mut notices = Vec::new();
    let mut planned: BTreeSet<String> = BTreeSet::new();
    let mut requests = Vec::new();
    for index in indices {
        let Some(entry) = har.log.entries.get(index) else {
            notices.push(format!("entry {index} is not in the HAR file"));
            continue;
        };
        let mut request = entry_request(&entry.request, &mut alloc, &mut notices);
        let name = unique_name(&request_name(&entry.request), |stem| {
            existing_names.contains(&format!("{stem}.bru")) || planned.contains(stem)
        });
        planned.insert(sanitize_name(&name).to_lowercase());
        request.name = name.clone();
        request.seq = start_seq.floor() + requests.len() as f64 + 1.0;
        requests.push((name, request));
    }
    ImportPlan {
        requests,
        new_secrets: alloc.new,
        notices,
    }
}

/// Display names are cut to this many characters (Plover's call), before uniqueness.
const MAX_DISPLAY_NAME: usize = 100;

/// `<METHOD> <path without the leading />` with `/` as `-`, or the host for `/`.
fn request_name(req: &HarRequest) -> String {
    let (host, path) = host_and_path(&req.url);
    let path = path.trim_matches('/').replace('/', "-");
    let base = if path.is_empty() { host } else { path };
    let base = if base.is_empty() {
        "request".to_owned()
    } else {
        base
    };
    let name = format!("{} {base}", req.method.to_ascii_uppercase());
    let cut: String = name.chars().take(MAX_DISPLAY_NAME).collect();
    cut.trim_end().to_owned()
}

struct SecretAlloc<'a> {
    existing: &'a ExistingSecrets,
    new: BTreeMap<String, String>,
}

impl SecretAlloc<'_> {
    /// The variable for `value` in the family `base`, `base2`, `base3`, ...: one that
    /// already holds `value`, or the first free name.
    fn var(&mut self, base: &str, value: &str) -> String {
        for n in 1.. {
            let name = if n == 1 {
                base.to_owned()
            } else {
                format!("{base}{n}")
            };
            if self.existing.plain_names.contains(&name) {
                continue;
            }
            if let Some(held) = self
                .existing
                .values
                .get(&name)
                .or_else(|| self.new.get(&name))
            {
                if held == value {
                    return name;
                }
                continue;
            }
            self.new.insert(name.clone(), value.to_owned());
            return name;
        }
        unreachable!("an unbounded range always finds a name")
    }

    fn placeholder(&mut self, base: &str, value: &str) -> String {
        format!("{{{{{}}}}}", self.var(base, value))
    }
}

/// `X-Api-Key` → `xApiKey`, `client_secret` → `clientSecret`.
fn camel_case(name: &str) -> String {
    let mut out = String::new();
    for (i, part) in name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|p| !p.is_empty())
        .enumerate()
    {
        let lower = part.to_ascii_lowercase();
        if i == 0 {
            out.push_str(&lower);
        } else {
            let mut chars = lower.chars();
            if let Some(first) = chars.next() {
                out.push(first.to_ascii_uppercase());
                out.extend(chars);
            }
        }
    }
    if out.is_empty() {
        "secret".to_owned()
    } else {
        out
    }
}

/// Headers the executor sets itself, and HTTP/2 pseudo-headers.
fn dropped_header(lower: &str) -> bool {
    lower.starts_with(':')
        || matches!(
            lower,
            "host" | "content-length" | "connection" | "accept-encoding"
        )
}

fn entry_request(
    har: &HarRequest,
    alloc: &mut SecretAlloc<'_>,
    notices: &mut Vec<String>,
) -> HttpRequest {
    let mut req = HttpRequest::blank();
    req.method = Method::from_block_name(&har.method.to_ascii_lowercase())
        .unwrap_or_else(|| Method::Custom(har.method.to_ascii_uppercase()));
    let mut authorization_seen = false;
    for h in &har.headers {
        let lower = h.name.to_ascii_lowercase();
        if dropped_header(&lower) {
            continue;
        }
        if h.value.is_empty() {
            req.headers.push(KeyValue::new(&h.name, ""));
            continue;
        }
        if lower == "authorization" && !authorization_seen {
            authorization_seen = true;
            match authorization(&h.value, alloc) {
                Some(auth) => req.auth = auth,
                None => {
                    req.auth = Auth::None;
                    let var = alloc.placeholder("authorization", &h.value);
                    req.headers.push(KeyValue::new(&h.name, var));
                }
            }
            continue;
        }
        let secret_base = if lower == "cookie" {
            Some("cookie".to_owned())
        } else if lower == "authorization" || is_token_header(&lower) || looks_like_jwt(&h.value) {
            Some(camel_case(&h.name))
        } else {
            None
        };
        let value = match secret_base {
            Some(base) => alloc.placeholder(&base, &h.value),
            None => h.value.clone(),
        };
        req.headers.push(KeyValue::new(&h.name, value));
    }
    req.url = url_with_query_secrets(&har.url, alloc);
    req.query = query_from_url(&req.url);
    req.body = entry_body(har, alloc, notices);
    req
}

/// `Bearer` and `Basic` become the request's auth with secret variables; any other
/// scheme gives `None` and the header is kept as a variable.
fn authorization(value: &str, alloc: &mut SecretAlloc<'_>) -> Option<Auth> {
    let (scheme, rest) = value.trim().split_once(char::is_whitespace)?;
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    if scheme.eq_ignore_ascii_case("bearer") {
        return Some(Auth::Bearer {
            token: alloc.placeholder("authToken", rest),
        });
    }
    if scheme.eq_ignore_ascii_case("basic") {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(rest)
            .ok()?;
        let text = String::from_utf8(decoded).ok()?;
        let (user, pass) = text.split_once(':')?;
        return Some(Auth::Basic {
            username: alloc.placeholder("authUser", user),
            password: alloc.placeholder("authPassword", pass),
        });
    }
    None
}

fn url_with_query_secrets(url: &str, alloc: &mut SecretAlloc<'_>) -> String {
    let Some((base, rest)) = url.split_once('?') else {
        return url.to_owned();
    };
    let (query, fragment) = match rest.split_once('#') {
        Some((q, f)) => (q, Some(f)),
        None => (rest, None),
    };
    let pairs: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, value))
                if !value.is_empty() && (is_token_query_param(name) || looks_like_jwt(value)) =>
            {
                format!("{name}={}", alloc.placeholder(&camel_case(name), value))
            }
            _ => pair.to_owned(),
        })
        .collect();
    let mut out = format!("{base}?{}", pairs.join("&"));
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    out
}

fn entry_body(har: &HarRequest, alloc: &mut SecretAlloc<'_>, notices: &mut Vec<String>) -> Body {
    let Some(pd) = &har.post_data else {
        return Body::None;
    };
    let header_type = har
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.clone());
    let mime = pd
        .mime_type
        .clone()
        .filter(|m| !m.trim().is_empty())
        .or(header_type);
    let lower = mime.as_deref().unwrap_or_default().to_ascii_lowercase();
    if lower.contains("multipart/form-data") && !pd.params.is_empty() {
        let fields = pd
            .params
            .iter()
            .map(|p| {
                let value = match &p.file_name {
                    Some(file) => {
                        notices.push(format!(
                            "{} {}: the file `{file}` in part `{}` is not in the HAR file; pick it again",
                            har.method, har.url, p.name
                        ));
                        MultipartValue::Files(vec![String::new()])
                    }
                    None => MultipartValue::Text(credential_value(
                        &p.name,
                        p.value.as_deref().unwrap_or_default(),
                        alloc,
                    )),
                };
                MultipartField {
                    name: p.name.clone(),
                    value,
                    enabled: true,
                    content_type: p.file_name.as_ref().and(p.content_type.clone()),
                }
            })
            .collect();
        return Body::Multipart(fields);
    }
    let body = match pd.text.as_deref().filter(|t| !t.is_empty()) {
        Some(text) => body_from_text(mime.as_deref(), text),
        None if !pd.params.is_empty() => Body::FormUrlEncoded(
            pd.params
                .iter()
                .map(|p| KeyValue::new(&p.name, p.value.as_deref().unwrap_or_default()))
                .collect(),
        ),
        None => Body::None,
    };
    match body {
        Body::FormUrlEncoded(fields) => Body::FormUrlEncoded(
            fields
                .into_iter()
                .map(|mut f| {
                    f.value = credential_value(&f.name, &f.value, alloc);
                    f
                })
                .collect(),
        ),
        Body::Json(text) => Body::Json(text_with_secrets(
            &json_with_secrets(&text, alloc),
            alloc,
            |kind| kind == SpanKind::Jwt,
        )),
        Body::Xml(text) => Body::Xml(text_with_secrets(&text, alloc, |_| true)),
        Body::Text(text) => Body::Text(text_with_secrets(&text, alloc, |_| true)),
        other => other,
    }
}

/// Replaces the credential spans of `text` whose kind `take` accepts with `{{var}}`,
/// keeping every other byte. A span's secret is its text as written (XML entities are
/// not decoded), so filling the variables back in gives the captured bytes. Variables
/// are allocated in document order.
fn text_with_secrets(
    text: &str,
    alloc: &mut SecretAlloc<'_>,
    take: impl Fn(SpanKind) -> bool,
) -> String {
    let spans: Vec<_> = body_credential_spans(text)
        .into_iter()
        .filter(|s| take(s.kind))
        .collect();
    let vars: Vec<String> = spans
        .iter()
        .map(|s| {
            let base = s
                .name
                .as_deref()
                .map_or_else(|| "bodyToken".to_owned(), camel_case);
            alloc.placeholder(&base, &text[s.range.clone()])
        })
        .collect();
    let mut out = text.to_owned();
    for (span, var) in spans.iter().zip(vars).rev() {
        out.replace_range(span.range.clone(), &var);
    }
    out
}

fn credential_value(name: &str, value: &str, alloc: &mut SecretAlloc<'_>) -> String {
    if is_credential_field(name) && !value.is_empty() {
        alloc.placeholder(&camel_case(name), value)
    } else {
        value.to_owned()
    }
}

/// `(key, value)` of every credential-named key at any depth whose value is a string.
fn json_credentials(value: &serde_json::Value, out: &mut Vec<(String, String)>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, v) in map {
                match v {
                    serde_json::Value::String(s) if is_credential_field(key) && !s.is_empty() => {
                        out.push((key.clone(), s.clone()))
                    }
                    _ => json_credentials(v, out),
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                json_credentials(item, out);
            }
        }
        _ => {}
    }
}

/// Replaces credential string values in a JSON text with `"{{var}}"`, keeping the rest
/// of the text as written. Falls back to re-serializing the whole value when a pair
/// cannot be found textually (unusual escapes).
fn json_with_secrets(text: &str, alloc: &mut SecretAlloc<'_>) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) else {
        return text.to_owned();
    };
    let mut pairs = Vec::new();
    json_credentials(&value, &mut pairs);
    if pairs.is_empty() {
        return text.to_owned();
    }
    let mut out = text.to_owned();
    let mut textual = true;
    let mut vars = Vec::new();
    for (key, secret) in &pairs {
        let var = alloc.var(&camel_case(key), secret);
        vars.push((key.clone(), secret.clone(), var.clone()));
        let (Ok(k), Ok(v)) = (serde_json::to_string(key), serde_json::to_string(secret)) else {
            textual = false;
            continue;
        };
        let pattern = format!(r"({}\s*:\s*){}", regex::escape(&k), regex::escape(&v));
        let Ok(re) = regex::Regex::new(&pattern) else {
            textual = false;
            continue;
        };
        if !re.is_match(&out) {
            textual = false;
            continue;
        }
        out = re
            .replace_all(&out, |caps: &regex::Captures<'_>| {
                format!("{}\"{{{{{var}}}}}\"", &caps[1])
            })
            .into_owned();
    }
    if textual {
        return out;
    }
    replace_json(&mut value, &vars);
    serde_json::to_string(&value).unwrap_or(out)
}

fn replace_json(value: &mut serde_json::Value, vars: &[(String, String, String)]) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if let serde_json::Value::String(s) = v
                    && let Some((_, _, var)) = vars.iter().find(|(k, val, _)| k == key && val == s)
                {
                    *s = format!("{{{{{var}}}}}");
                } else {
                    replace_json(v, vars);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                replace_json(item, vars);
            }
        }
        _ => {}
    }
}
