//! Sends a `ResolvedRequest` with reqwest (rustls). One cookie jar per `Session`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::redirect::Policy;
use reqwest_cookie_store::CookieStoreMutex;

use crate::resolve::{ResolvedBody, ResolvedPart, ResolvedRequest};

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
        for (name, value) in &req.headers {
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| ExecError::InvalidHeader(name.clone()))?;
            let value = reqwest::header::HeaderValue::from_str(value)
                .map_err(|_| ExecError::InvalidHeader(name.to_string()))?;
            builder = builder.header(name, value);
        }
        builder = match &req.body {
            ResolvedBody::None => builder,
            ResolvedBody::Bytes(data) => builder.body(data.clone()),
            ResolvedBody::Form(pairs) => builder.form(pairs),
            ResolvedBody::Multipart(parts) => builder.multipart(multipart_form(parts).await?),
        };

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
        let policy = if follow {
            Policy::limited(max as usize)
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

async fn multipart_form(parts: &[ResolvedPart]) -> Result<reqwest::multipart::Form, ExecError> {
    let mut form = reqwest::multipart::Form::new();
    for part in parts {
        let (name, mut built, content_type) = match part {
            ResolvedPart::Text {
                name,
                value,
                content_type,
            } => (
                name,
                reqwest::multipart::Part::text(value.clone()),
                content_type,
            ),
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
                (
                    name,
                    reqwest::multipart::Part::bytes(data).file_name(file_name),
                    content_type,
                )
            }
        };
        if let Some(ct) = content_type {
            built = built
                .mime_str(ct)
                .map_err(|_| ExecError::InvalidContentType(ct.clone()))?;
        }
        form = form.part(name.clone(), built);
    }
    Ok(form)
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
