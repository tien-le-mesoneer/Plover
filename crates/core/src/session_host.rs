//! What scripts ask of the run's session: HTTP for `bru.sendRequest` / `axios` and the
//! cookie jar for `bru.cookies`. The script engine forwards these (`ScriptHost`); the runner
//! serves them with the session it already holds.

use std::sync::{Mutex, PoisonError};

use base64::Engine as _;
use reqwest_cookie_store::RawCookie;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use crate::executor::{ExecError, Session};
use crate::request::{Method, RequestSettings};
use crate::resolve::{ResolvedBody, ResolvedRequest};
use crate::script::{HostFuture, ScriptHost};

/// A request a script sent, for the results (Bruno records these as scripted requests).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptedRequest {
    /// `sendRequest` or `axios`.
    pub source: String,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

pub struct SessionHost<'a> {
    session: &'a Session,
    recorded: Mutex<Vec<ScriptedRequest>>,
}

impl<'a> SessionHost<'a> {
    pub fn new(session: &'a Session) -> SessionHost<'a> {
        SessionHost {
            session,
            recorded: Mutex::default(),
        }
    }

    /// The requests scripts sent so far, in order.
    pub fn take_recorded(&self) -> Vec<ScriptedRequest> {
        std::mem::take(&mut *self.recorded.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl ScriptHost for SessionHost<'_> {
    fn call<'a>(&'a self, op: &'a str, args: Value) -> HostFuture<'a> {
        Box::pin(async move {
            match op {
                "http" => self.http(args).await,
                "cookies.list" => self.cookies_list(&args),
                "cookies.get" => self.cookies_get(&args),
                "cookies.set" => self.cookies_set(&args),
                "cookies.delete" => self.cookies_delete(&args),
                "cookies.clear" => {
                    self.jar().clear();
                    Ok(Value::Null)
                }
                other => Err(format!("unknown host operation {other}")),
            }
        })
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct HttpArgs {
    source: String,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<HttpBody>,
    timeout_ms: Option<u64>,
    max_redirects: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct HttpBody {
    text: Option<String>,
    base64: Option<String>,
}

/// axios-style error codes for transport failures.
fn error_code(e: &ExecError) -> &'static str {
    match e {
        ExecError::Timeout => "ECONNABORTED",
        ExecError::Transport(m) if m.contains("onnection refused") => "ECONNREFUSED",
        ExecError::Transport(m) if m.contains("dns error") => "ENOTFOUND",
        ExecError::Transport(m) if m.contains("builder error") => "ERR_INVALID_URL",
        ExecError::InvalidMethod(_) | ExecError::InvalidHeader(_) => "ERR_BAD_OPTION_VALUE",
        _ => "ERR_NETWORK",
    }
}

impl SessionHost<'_> {
    fn jar(&self) -> std::sync::MutexGuard<'_, reqwest_cookie_store::CookieStore> {
        self.session
            .cookie_jar()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    async fn http(&self, args: Value) -> Result<Value, String> {
        let args: HttpArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let body = match args.body {
            None => ResolvedBody::None,
            Some(HttpBody {
                base64: Some(b64), ..
            }) => ResolvedBody::Bytes(
                base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| e.to_string())?,
            ),
            Some(HttpBody { text, .. }) => {
                ResolvedBody::Bytes(text.unwrap_or_default().into_bytes())
            }
        };
        let method = Method::from_block_name(&args.method.to_ascii_lowercase())
            .unwrap_or_else(|| Method::Custom(args.method.clone()));
        let req = ResolvedRequest {
            method,
            url: args.url.clone(),
            headers: args.headers,
            body,
            settings: RequestSettings {
                follow_redirects: None,
                max_redirects: args.max_redirects,
                timeout_ms: args.timeout_ms,
            },
        };
        let started = std::time::Instant::now();
        let result = self.session.send_without_cookies(&req).await;
        let mut record = ScriptedRequest {
            source: args.source,
            method: args.method,
            url: args.url,
            status: None,
            error: None,
            duration_ms: started.elapsed().as_millis() as u64,
        };
        let answer = match result {
            Ok(res) => {
                record.status = Some(res.status);
                json!({
                    "ok": true,
                    "status": res.status,
                    "statusText": reqwest::StatusCode::from_u16(res.status)
                        .ok()
                        .and_then(|s| s.canonical_reason())
                        .unwrap_or_default(),
                    "headers": res.headers,
                    "text": String::from_utf8_lossy(&res.body),
                    "durationMs": res.elapsed.as_millis() as u64,
                })
            }
            Err(e) => {
                let message = match (&e, req.settings.timeout_ms) {
                    (ExecError::Timeout, Some(ms)) => format!("timeout of {ms}ms exceeded"),
                    _ => e.to_string(),
                };
                record.error = Some(message.clone());
                json!({ "ok": false, "error": { "message": message, "code": error_code(&e) } })
            }
        };
        self.recorded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(record);
        Ok(answer)
    }

    fn cookies_list(&self, args: &Value) -> Result<Value, String> {
        let url = parse_url(args)?;
        let jar = self.jar();
        Ok(Value::Array(
            sorted_matches(&jar, &url)
                .into_iter()
                .map(|c| cookie_json(c, &url))
                .collect(),
        ))
    }

    fn cookies_get(&self, args: &Value) -> Result<Value, String> {
        let url = parse_url(args)?;
        let name = text(args, "name");
        let jar = self.jar();
        Ok(sorted_matches(&jar, &url)
            .into_iter()
            .find(|c| c.name() == name)
            .map(|c| cookie_json(c, &url))
            .unwrap_or(Value::Null))
    }

    /// `{ key, value, domain?, path?, expires?, maxAge?, secure?, httpOnly?, sameSite? }`.
    fn cookies_set(&self, args: &Value) -> Result<Value, String> {
        let url = parse_url(args)?;
        let c = args.get("cookie").cloned().unwrap_or(Value::Null);
        let field = |k: &str| c.get(k).and_then(Value::as_str);
        let key = field("key").or_else(|| field("name")).unwrap_or_default();
        if key.is_empty() {
            return Err("cookie name is required".to_owned());
        }
        let value = c
            .get("value")
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
            .unwrap_or_default();
        let mut header = format!("{key}={value}");
        if let Some(domain) = field("domain") {
            header.push_str(&format!("; Domain={domain}"));
        }
        header.push_str(&format!("; Path={}", field("path").unwrap_or("/")));
        if let Some(max_age) = c.get("maxAge").and_then(Value::as_i64) {
            header.push_str(&format!("; Max-Age={max_age}"));
        }
        if let Some(expires) = field("expires").filter(|e| *e != "Infinity") {
            header.push_str(&format!("; Expires={expires}"));
        }
        if c.get("secure").and_then(Value::as_bool) == Some(true) {
            header.push_str("; Secure");
        }
        if c.get("httpOnly").and_then(Value::as_bool) == Some(true) {
            header.push_str("; HttpOnly");
        }
        if let Some(same_site) = field("sameSite") {
            header.push_str(&format!("; SameSite={same_site}"));
        }
        let raw = RawCookie::parse(header).map_err(|e| format!("invalid cookie: {e}"))?;
        self.jar()
            .insert_raw(&raw, &url)
            .map_err(|e| format!("cannot set cookie: {e}"))?;
        Ok(Value::Null)
    }

    /// Removes the cookies matching `url` (all, or those named `name`).
    fn cookies_delete(&self, args: &Value) -> Result<Value, String> {
        let url = parse_url(args)?;
        let name = args.get("name").and_then(Value::as_str);
        let mut jar = self.jar();
        let targets: Vec<(String, String, String)> = jar
            .matches(&url)
            .into_iter()
            .filter(|c| name.is_none_or(|n| c.name() == n))
            .map(|c| {
                (
                    cookie_domain(c, &url),
                    String::from(&c.path),
                    c.name().to_owned(),
                )
            })
            .collect();
        for (domain, path, name) in targets {
            jar.remove(&domain, &path, &name);
        }
        Ok(Value::Null)
    }
}

/// Cookies for `url`, longest path first (RFC 6265 5.4); ties by name. tough-cookie
/// breaks ties by creation time, which `cookie_store` does not keep.
fn sorted_matches<'a>(
    jar: &'a reqwest_cookie_store::CookieStore,
    url: &Url,
) -> Vec<&'a cookie_store::Cookie<'static>> {
    let mut cookies = jar.matches(url);
    cookies.sort_by(|a, b| {
        String::from(&b.path)
            .len()
            .cmp(&String::from(&a.path).len())
            .then_with(|| a.name().cmp(b.name()))
    });
    cookies
}

fn text<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn parse_url(args: &Value) -> Result<Url, String> {
    let raw = text(args, "url");
    Url::parse(raw).map_err(|e| format!("Invalid URL {raw}: {e}"))
}

fn cookie_domain(c: &cookie_store::Cookie<'_>, url: &Url) -> String {
    c.domain
        .as_cow()
        .map(|d| d.into_owned())
        .unwrap_or_else(|| url.host_str().unwrap_or_default().to_owned())
}

/// Plain cookie data; `api.js` shapes it like tough-cookie or cookie-list.js.
fn cookie_json(c: &cookie_store::Cookie<'_>, url: &Url) -> Value {
    let expires_ms = match c.expires {
        cookie_store::CookieExpiration::AtUtc(t) => Some(t.unix_timestamp() * 1000),
        cookie_store::CookieExpiration::SessionEnd => None,
    };
    json!({
        "key": c.name(),
        "value": c.value(),
        "domain": cookie_domain(c, url),
        "path": String::from(&c.path),
        "secure": c.secure().unwrap_or(false),
        "httpOnly": c.http_only().unwrap_or(false),
        "hostOnly": matches!(c.domain, cookie_store::CookieDomain::HostOnly(_)),
        "expiresMs": expires_ms,
    })
}
