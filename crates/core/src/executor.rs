//! Sends a `ResolvedRequest` with reqwest (rustls). One cookie jar per `Session`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::redirect::Policy;
use reqwest_cookie_store::CookieStoreMutex;

use crate::resolve::{ResolvedBody, ResolvedPart, ResolvedRequest, fold_case_variants};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOptions {
    /// `None` means no timeout (Bruno's default).
    pub timeout: Option<Duration>,
    pub follow_redirects: bool,
    pub max_redirects: u32,
    pub verify_tls: bool,
}

impl Default for ExecOptions {
    fn default() -> Self {
        ExecOptions {
            timeout: None,
            follow_redirects: true,
            max_redirects: 5,
            verify_tls: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResponseSummary {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// From sending the request until the whole body was read.
    pub elapsed: Duration,
    pub size: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("request timed out")]
    Timeout,
    #[error("invalid method `{0}`")]
    InvalidMethod(String),
    #[error("invalid header `{0}`")]
    InvalidHeader(String),
    #[error("invalid content type `{0}`")]
    InvalidContentType(String),
    #[error("cannot read file {path}: {source}")]
    File {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot build HTTP client: {0}")]
    Client(String),
    #[error("{0}")]
    Transport(String),
}

/// Shared state for one run: options, the cookie jar, and clients per redirect policy.
pub struct Session {
    opts: ExecOptions,
    jar: Arc<CookieStoreMutex>,
    clients: Mutex<HashMap<(bool, u32), reqwest::Client>>,
}

impl Session {
    pub fn new(opts: ExecOptions) -> Result<Session, ExecError> {
        let session = Session {
            opts,
            jar: Arc::default(),
            clients: Mutex::default(),
        };
        session.client(session.opts.follow_redirects, session.opts.max_redirects)?;
        Ok(session)
    }

    pub async fn send(&self, req: &ResolvedRequest) -> Result<ResponseSummary, ExecError> {
        let follow = req
            .settings
            .follow_redirects
            .unwrap_or(self.opts.follow_redirects);
        let max = req
            .settings
            .max_redirects
            .unwrap_or(self.opts.max_redirects);
        let client = self.client(follow, max)?;
        let method = reqwest::Method::from_bytes(req.method.as_str().as_bytes())
            .map_err(|_| ExecError::InvalidMethod(req.method.as_str().to_owned()))?;
        let mut builder = client.request(method, &req.url);
        let timeout = req
            .settings
            .timeout_ms
            .filter(|ms| *ms > 0)
            .map(Duration::from_millis)
            .or(self.opts.timeout);
        if let Some(timeout) = timeout {
            builder = builder.timeout(timeout);
        }
        // axios-instance.js:101-111: axios' common Accept default plus a User-Agent,
        // folded with the request's own headers so a user value (any case) wins.
        let mut headers = vec![
            ("Accept".to_owned(), DEFAULT_ACCEPT.to_owned()),
            ("User-Agent".to_owned(), user_agent()),
        ];
        headers.extend(req.headers.iter().cloned());
        let mut headers = fold_case_variants(headers);
        let body = match &req.body {
            ResolvedBody::None => None,
            ResolvedBody::Bytes(data) => Some(data.clone()),
            ResolvedBody::Form(_) => None,
            ResolvedBody::Multipart(parts) => {
                let boundary = multipart_content_type(&mut headers);
                Some(encode_multipart(parts, &boundary).await?)
            }
        };
        for (name, value) in &headers {
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| ExecError::InvalidHeader(name.clone()))?;
            let value = reqwest::header::HeaderValue::from_str(value)
                .map_err(|_| ExecError::InvalidHeader(name.to_string()))?;
            builder = builder.header(name, value);
        }
        if let ResolvedBody::Form(pairs) = &req.body {
            builder = builder.form(pairs);
        }
        if let Some(body) = body {
            builder = builder.body(body);
        }

        let start = Instant::now();
        let response = builder.send().await.map_err(transport_error)?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect();
        let body = response.bytes().await.map_err(transport_error)?.to_vec();
        let elapsed = start.elapsed();
        Ok(ResponseSummary {
            status,
            headers,
            size: body.len(),
            body,
            elapsed,
        })
    }

    fn client(&self, follow: bool, max: u32) -> Result<reqwest::Client, ExecError> {
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| ExecError::Client("client cache poisoned".into()))?;
        if let Some(client) = clients.get(&(follow, max)) {
            return Ok(client.clone());
        }
        // axios-instance.js:166-169: once `max` redirects were followed, the next 3xx
        // is returned as the response instead of failing.
        let policy = if follow {
            let max = max as usize;
            Policy::custom(move |attempt| {
                if attempt.previous().len() > max {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            })
        } else {
            Policy::none()
        };
        let client = reqwest::Client::builder()
            .redirect(policy)
            .cookie_provider(self.jar.clone())
            .tls_danger_accept_invalid_certs(!self.opts.verify_tls)
            .build()
            .map_err(|e| ExecError::Client(e.to_string()))?;
        clients.insert((follow, max), client.clone());
        Ok(client)
    }
}

const DEFAULT_ACCEPT: &str = "application/json, text/plain, */*";

fn user_agent() -> String {
    format!("bru-rs/{}", env!("CARGO_PKG_VERSION"))
}

/// run-single-request.js:555-575: reuse a boundary already in the content type,
/// otherwise append `; boundary=...` to it (or to `multipart/form-data`). Returns the boundary.
fn multipart_content_type(headers: &mut Vec<(String, String)>) -> String {
    let index = match headers
        .iter()
        .position(|(k, _)| k.eq_ignore_ascii_case("content-type"))
    {
        Some(i) => i,
        None => {
            headers.push(("content-type".to_owned(), "multipart/form-data".to_owned()));
            headers.len() - 1
        }
    };
    let value = &mut headers[index].1;
    if let Some(existing) = boundary_param(value) {
        return existing;
    }
    let boundary = new_boundary();
    *value = format!("{value}; boundary={boundary}");
    boundary
}

/// bruno-common `extractBoundaryFromContentType`: the `boundary` parameter, unquoted.
fn boundary_param(content_type: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|param| {
        let (key, value) = param.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case("boundary")
            .then(|| value.trim().trim_matches('"').to_owned())
    })
}

/// `form-data` style: 26 dashes and 24 digits.
fn new_boundary() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let n = (nanos as u64) ^ COUNTER.fetch_add(1, Ordering::Relaxed).rotate_left(32);
    format!("{}{:024}", "-".repeat(26), n)
}

/// `multipart/form-data` body (RFC 7578) as the `form-data` package writes it. File
/// parts without an explicit type get `application/octet-stream`.
async fn encode_multipart(parts: &[ResolvedPart], boundary: &str) -> Result<Vec<u8>, ExecError> {
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let (data, content_type) = match part {
            ResolvedPart::Text {
                name,
                value,
                content_type,
            } => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{}\"\r\n",
                        escape_quoted(name)
                    )
                    .as_bytes(),
                );
                (value.clone().into_bytes(), content_type.clone())
            }
            ResolvedPart::File {
                name,
                path,
                content_type,
            } => {
                let data = tokio::fs::read(path)
                    .await
                    .map_err(|source| ExecError::File {
                        path: path.clone(),
                        source,
                    })?;
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
                        escape_quoted(name),
                        escape_quoted(&file_name)
                    )
                    .as_bytes(),
                );
                (
                    data,
                    Some(
                        content_type
                            .clone()
                            .unwrap_or_else(|| "application/octet-stream".to_owned()),
                    ),
                )
            }
        };
        if let Some(ct) = content_type {
            body.extend_from_slice(format!("Content-Type: {ct}\r\n").as_bytes());
        }
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(&data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok(body)
}

fn escape_quoted(s: &str) -> String {
    s.replace('"', "%22")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn transport_error(e: reqwest::Error) -> ExecError {
    if e.is_timeout() {
        return ExecError::Timeout;
    }
    let mut message = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(s) = source {
        message = format!("{message}: {s}");
        source = s.source();
    }
    ExecError::Transport(message)
}
