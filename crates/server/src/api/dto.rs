//! Wire types for the REST and WebSocket API. Field names are camelCase;
//! TypeScript definitions are generated from these with ts-rs.

use std::collections::BTreeMap;

use brs_core::request::{
    Auth, Body, HttpRequest, KeyValue, Method, MultipartField, MultipartValue, RequestSettings,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct KeyValueDto {
    pub name: String,
    pub value: String,
    pub enabled: bool,
    /// `@name` request-local variable (vars blocks only).
    #[serde(default)]
    pub local: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MultipartValueDto {
    Text { value: String },
    Files { paths: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MultipartFieldDto {
    pub name: String,
    pub value: MultipartValueDto,
    pub enabled: bool,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum BodyDto {
    None,
    Json {
        text: String,
    },
    Text {
        text: String,
    },
    Xml {
        text: String,
    },
    FormUrlEncoded {
        fields: Vec<KeyValueDto>,
    },
    MultipartForm {
        fields: Vec<MultipartFieldDto>,
    },
    /// A body mode this version keeps but does not send (graphql, sparql, file, ...).
    Unsupported {
        name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum AuthDto {
    None,
    Inherit,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    /// An auth mode this version keeps but does not apply (oauth2, awsv4, ...).
    Unsupported {
        name: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RequestSettingsDto {
    pub follow_redirects: Option<bool>,
    pub max_redirects: Option<u32>,
    #[ts(type = "number | null")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequestDto {
    pub name: String,
    pub seq: f64,
    /// `meta.type`, usually `http`.
    pub kind: String,
    /// Uppercase method name; any other string is a custom method.
    pub method: String,
    pub url: String,
    pub query: Vec<KeyValueDto>,
    pub path_params: Vec<KeyValueDto>,
    pub headers: Vec<KeyValueDto>,
    pub body: BodyDto,
    pub auth: AuthDto,
    pub vars_pre: Vec<KeyValueDto>,
    pub vars_post: Vec<KeyValueDto>,
    pub script_pre: Option<String>,
    pub script_post: Option<String>,
    pub tests: Option<String>,
    pub settings: RequestSettingsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSummaryDto {
    pub id: String,
    pub name: String,
    pub root: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TreeNodeKind {
    Folder,
    Request,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TreeNodeDto {
    pub kind: TreeNodeKind,
    /// `/`-separated path relative to the collection root; `""` for the root.
    pub id: String,
    pub name: String,
    pub method: Option<String>,
    pub seq: Option<f64>,
    /// Parse error of an invalid file.
    pub error: Option<String>,
    /// Present on folders only.
    pub children: Option<Vec<TreeNodeDto>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDto {
    pub name: String,
    pub vars: Vec<KeyValueDto>,
    pub secret_names: Vec<String>,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResponseDto {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    /// Decoded body length in bytes, even when the body is truncated.
    #[ts(type = "number")]
    pub size: usize,
    /// The body when it is valid UTF-8.
    pub body_text: Option<String>,
    /// The body otherwise.
    pub body_base64: Option<String>,
    /// The body was cut to the first 10 MiB.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OpenCollectionBody {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StoredRequestDto {
    pub request: HttpRequestDto,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SaveRequestBody {
    pub request: HttpRequestDto,
    /// Hash of the file as last read; a mismatch returns 409.
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HashDto {
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SendBody {
    pub item: String,
    #[serde(default)]
    pub draft: Option<HttpRequestDto>,
    #[serde(default)]
    pub env: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SaveEnvironmentBody {
    pub vars: Vec<KeyValueDto>,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SecretsBody {
    pub values: BTreeMap<String, String>,
}

/// The file content found on disk when a save conflicts.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConflictDto {
    pub text: String,
    pub hash: String,
}

/// Body of every non-2xx API response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorDto {
    pub error: String,
    pub message: String,
    /// Present on 409 `conflict`.
    pub current: Option<ConflictDto>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunReportDto {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    #[ts(type = "number")]
    pub elapsed_ms: u64,
}

impl From<&KeyValue> for KeyValueDto {
    fn from(kv: &KeyValue) -> Self {
        KeyValueDto {
            name: kv.name.clone(),
            value: kv.value.clone(),
            enabled: kv.enabled,
            local: kv.local,
        }
    }
}

impl From<KeyValueDto> for KeyValue {
    fn from(kv: KeyValueDto) -> Self {
        KeyValue {
            name: kv.name,
            value: kv.value,
            enabled: kv.enabled,
            local: kv.local,
        }
    }
}

fn kvs(list: &[KeyValue]) -> Vec<KeyValueDto> {
    list.iter().map(KeyValueDto::from).collect()
}

fn from_kvs(list: Vec<KeyValueDto>) -> Vec<KeyValue> {
    list.into_iter().map(KeyValue::from).collect()
}

impl From<&HttpRequest> for HttpRequestDto {
    fn from(r: &HttpRequest) -> Self {
        HttpRequestDto {
            name: r.name.clone(),
            seq: r.seq,
            kind: r.kind.clone(),
            method: r.method.as_str().to_owned(),
            url: r.url.clone(),
            query: kvs(&r.query),
            path_params: kvs(&r.path_params),
            headers: kvs(&r.headers),
            body: match &r.body {
                Body::None => BodyDto::None,
                Body::Json(text) => BodyDto::Json { text: text.clone() },
                Body::Text(text) => BodyDto::Text { text: text.clone() },
                Body::Xml(text) => BodyDto::Xml { text: text.clone() },
                Body::FormUrlEncoded(fields) => BodyDto::FormUrlEncoded {
                    fields: kvs(fields),
                },
                Body::Multipart(fields) => BodyDto::MultipartForm {
                    fields: fields
                        .iter()
                        .map(|f| MultipartFieldDto {
                            name: f.name.clone(),
                            value: match &f.value {
                                MultipartValue::Text(value) => MultipartValueDto::Text {
                                    value: value.clone(),
                                },
                                MultipartValue::Files(paths) => MultipartValueDto::Files {
                                    paths: paths.clone(),
                                },
                            },
                            enabled: f.enabled,
                            content_type: f.content_type.clone(),
                        })
                        .collect(),
                },
                Body::Unsupported(name) => BodyDto::Unsupported { name: name.clone() },
            },
            auth: match &r.auth {
                Auth::None => AuthDto::None,
                Auth::Inherit => AuthDto::Inherit,
                Auth::Basic { username, password } => AuthDto::Basic {
                    username: username.clone(),
                    password: password.clone(),
                },
                Auth::Bearer { token } => AuthDto::Bearer {
                    token: token.clone(),
                },
                Auth::Unsupported(name) => AuthDto::Unsupported { name: name.clone() },
            },
            vars_pre: kvs(&r.vars_pre),
            vars_post: kvs(&r.vars_post),
            script_pre: r.script_pre.clone(),
            script_post: r.script_post.clone(),
            tests: r.tests.clone(),
            settings: RequestSettingsDto {
                follow_redirects: r.settings.follow_redirects,
                max_redirects: r.settings.max_redirects,
                timeout_ms: r.settings.timeout_ms,
            },
        }
    }
}

impl From<HttpRequestDto> for HttpRequest {
    fn from(d: HttpRequestDto) -> Self {
        let method = Method::from_block_name(&d.method.to_ascii_lowercase())
            .unwrap_or(Method::Custom(d.method));
        HttpRequest {
            name: d.name,
            seq: d.seq,
            kind: d.kind,
            method,
            url: d.url,
            query: from_kvs(d.query),
            path_params: from_kvs(d.path_params),
            headers: from_kvs(d.headers),
            body: match d.body {
                BodyDto::None => Body::None,
                BodyDto::Json { text } => Body::Json(text),
                BodyDto::Text { text } => Body::Text(text),
                BodyDto::Xml { text } => Body::Xml(text),
                BodyDto::FormUrlEncoded { fields } => Body::FormUrlEncoded(from_kvs(fields)),
                BodyDto::MultipartForm { fields } => Body::Multipart(
                    fields
                        .into_iter()
                        .map(|f| MultipartField {
                            name: f.name,
                            value: match f.value {
                                MultipartValueDto::Text { value } => MultipartValue::Text(value),
                                MultipartValueDto::Files { paths } => MultipartValue::Files(paths),
                            },
                            enabled: f.enabled,
                            content_type: f.content_type,
                        })
                        .collect(),
                ),
                BodyDto::Unsupported { name } => Body::Unsupported(name),
            },
            auth: match d.auth {
                AuthDto::None => Auth::None,
                AuthDto::Inherit => Auth::Inherit,
                AuthDto::Basic { username, password } => Auth::Basic { username, password },
                AuthDto::Bearer { token } => Auth::Bearer { token },
                AuthDto::Unsupported { name } => Auth::Unsupported(name),
            },
            vars_pre: from_kvs(d.vars_pre),
            vars_post: from_kvs(d.vars_post),
            script_pre: d.script_pre,
            script_post: d.script_post,
            tests: d.tests,
            settings: RequestSettings {
                follow_redirects: d.settings.follow_redirects,
                max_redirects: d.settings.max_redirects,
                timeout_ms: d.settings.timeout_ms,
            },
        }
    }
}

/// All DTO declarations as one TypeScript module.
pub fn typescript_bindings() -> String {
    use ts_rs::Config;
    let cfg = Config::default();
    let decls = [
        KeyValueDto::decl(&cfg),
        MultipartValueDto::decl(&cfg),
        MultipartFieldDto::decl(&cfg),
        BodyDto::decl(&cfg),
        AuthDto::decl(&cfg),
        RequestSettingsDto::decl(&cfg),
        HttpRequestDto::decl(&cfg),
        CollectionSummaryDto::decl(&cfg),
        TreeNodeKind::decl(&cfg),
        TreeNodeDto::decl(&cfg),
        EnvironmentDto::decl(&cfg),
        ResponseDto::decl(&cfg),
        OpenCollectionBody::decl(&cfg),
        StoredRequestDto::decl(&cfg),
        SaveRequestBody::decl(&cfg),
        HashDto::decl(&cfg),
        SendBody::decl(&cfg),
        SaveEnvironmentBody::decl(&cfg),
        SecretsBody::decl(&cfg),
        ConflictDto::decl(&cfg),
        ApiErrorDto::decl(&cfg),
        RunReportDto::decl(&cfg),
        super::runs::RunEventDto::decl(&cfg),
        super::runs::ClientMessage::decl(&cfg),
    ];
    let mut out = String::from(
        "// Generated from crates/server/src/api by ts-rs. Do not edit.\n\
         // Regenerate: BRS_UPDATE_BINDINGS=1 cargo test -p brs-server --test bindings\n",
    );
    for decl in decls {
        out.push('\n');
        out.push_str("export ");
        out.push_str(&decl);
        out.push('\n');
    }
    out
}
