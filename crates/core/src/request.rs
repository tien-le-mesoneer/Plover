//! Request domain model, independent of the `.bru` encoding.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Options,
    Head,
    Connect,
    Trace,
    Custom(String),
}

impl Method {
    /// Lowercase `.bru` block name for standard methods; `None` for custom ones.
    pub fn block_name(&self) -> Option<&'static str> {
        Some(match self {
            Method::Get => "get",
            Method::Post => "post",
            Method::Put => "put",
            Method::Delete => "delete",
            Method::Patch => "patch",
            Method::Options => "options",
            Method::Head => "head",
            Method::Connect => "connect",
            Method::Trace => "trace",
            Method::Custom(_) => return None,
        })
    }

    pub fn from_block_name(name: &str) -> Option<Method> {
        Some(match name {
            "get" => Method::Get,
            "post" => Method::Post,
            "put" => Method::Put,
            "delete" => Method::Delete,
            "patch" => Method::Patch,
            "options" => Method::Options,
            "head" => Method::Head,
            "connect" => Method::Connect,
            "trace" => Method::Trace,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Patch => "PATCH",
            Method::Options => "OPTIONS",
            Method::Head => "HEAD",
            Method::Connect => "CONNECT",
            Method::Trace => "TRACE",
            Method::Custom(m) => m,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyValue {
    pub name: String,
    pub value: String,
    pub enabled: bool,
    /// `@name` in `vars:*` blocks: a request-local variable (Bruno `local: true`).
    pub local: bool,
}

impl KeyValue {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> KeyValue {
        KeyValue {
            name: name.into(),
            value: value.into(),
            enabled: true,
            local: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    None,
    Json(String),
    Text(String),
    Xml(String),
    FormUrlEncoded(Vec<KeyValue>),
    Multipart(Vec<MultipartField>),
    /// A body mode this version does not execute (graphql, sparql, file, ...).
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartField {
    pub name: String,
    pub value: MultipartValue,
    pub enabled: bool,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultipartValue {
    Text(String),
    /// `@file(a|b)`: paths relative to the collection root.
    Files(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    None,
    Inherit,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    /// An auth mode this version does not execute (oauth2, awsv4, digest, ...).
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestSettings {
    pub follow_redirects: Option<bool>,
    pub max_redirects: Option<u32>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub name: String,
    pub seq: f64,
    /// `meta.type`; `http` when absent.
    pub kind: String,
    pub method: Method,
    pub url: String,
    pub query: Vec<KeyValue>,
    pub path_params: Vec<KeyValue>,
    pub headers: Vec<KeyValue>,
    /// The active body (the HTTP block's `body:` mode).
    pub body: Body,
    /// Bodies of other modes present in the file. Bruno keeps one body per
    /// mode, so switching modes and back restores the earlier content.
    pub other_bodies: Vec<Body>,
    pub auth: Auth,
    pub vars_pre: Vec<KeyValue>,
    pub vars_post: Vec<KeyValue>,
    pub script_pre: Option<String>,
    pub script_post: Option<String>,
    pub tests: Option<String>,
    pub settings: RequestSettings,
}
