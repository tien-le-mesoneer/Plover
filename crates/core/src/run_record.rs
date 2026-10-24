//! What a run keeps of each request: the request as sent and the response as received
//! (Bruno CLI keeps both in each result, run-single-request.js).

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::executor::ResponseSummary;
use crate::resolve::{ResolvedBody, ResolvedPart, ResolvedRequest};

/// Each stored request or response body keeps at most this many bytes.
pub const MAX_DETAIL_BODY: usize = 1 << 20;

/// The request as it was sent, after variables and scripts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDetail {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: BodyDetail,
}

/// The response as the run saw it, with a body replaced by `res.setBody`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseDetail {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: BodyDetail,
    pub url: String,
    pub size: usize,
    /// `false` when the executor stopped reading the body, so `size` is a lower bound.
    pub size_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BodyDetail {
    None,
    /// UTF-8 text; `size` is the whole body's length in bytes.
    Text {
        text: String,
        size: usize,
        truncated: bool,
    },
    /// Bytes that are not UTF-8, base64-encoded.
    Base64 {
        data: String,
        size: usize,
        truncated: bool,
    },
    Form {
        fields: Vec<(String, String)>,
    },
    Multipart {
        parts: Vec<PartDetail>,
    },
    /// Not kept, e.g. after the run's body budget ran out.
    Omitted {
        reason: String,
        size: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PartDetail {
    #[serde(rename_all = "camelCase")]
    Text {
        name: String,
        value: String,
        content_type: Option<String>,
    },
    /// Only the file name is kept, never the file's bytes.
    #[serde(rename_all = "camelCase")]
    File {
        name: String,
        file_name: String,
        content_type: Option<String>,
    },
}

impl RequestDetail {
    pub fn of(resolved: &ResolvedRequest) -> RequestDetail {
        let body = match &resolved.body {
            ResolvedBody::None => BodyDetail::None,
            ResolvedBody::Bytes(bytes) => BodyDetail::of_bytes(bytes, bytes.len(), false),
            ResolvedBody::Form(fields) => BodyDetail::Form {
                fields: fields.clone(),
            },
            ResolvedBody::Multipart(parts) => BodyDetail::Multipart {
                parts: parts
                    .iter()
                    .map(|p| match p {
                        ResolvedPart::Text {
                            name,
                            value,
                            content_type,
                        } => PartDetail::Text {
                            name: name.clone(),
                            value: value.clone(),
                            content_type: content_type.clone(),
                        },
                        ResolvedPart::File {
                            name,
                            path,
                            content_type,
                        } => PartDetail::File {
                            name: name.clone(),
                            file_name: path
                                .file_name()
                                .map(|f| f.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            content_type: content_type.clone(),
                        },
                    })
                    .collect(),
            },
        };
        RequestDetail {
            method: resolved.method.as_str().to_owned(),
            url: resolved.url.clone(),
            headers: resolved.headers.clone(),
            body,
        }
    }
}

impl ResponseDetail {
    pub fn of(response: &ResponseSummary, url: &str) -> ResponseDetail {
        ResponseDetail {
            status: response.status,
            status_text: status_text(response.status),
            headers: response.headers.clone(),
            body: BodyDetail::of_bytes(&response.body, response.body.len(), response.truncated),
            url: url.to_owned(),
            size: response.size,
            size_complete: !response.truncated,
        }
    }
}

/// The standard reason phrase, as scripts see it in `res.statusText`.
pub fn status_text(status: u16) -> String {
    reqwest::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or_default()
        .to_owned()
}

impl BodyDetail {
    /// `size` is the whole body's length; `truncated` says `bytes` is already cut short.
    pub fn of_bytes(bytes: &[u8], size: usize, truncated: bool) -> BodyDetail {
        if bytes.is_empty() && !truncated {
            return BodyDetail::None;
        }
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                let mut end = text.len().min(MAX_DETAIL_BODY);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                BodyDetail::Text {
                    text: text[..end].to_owned(),
                    size,
                    truncated: truncated || end < text.len(),
                }
            }
            // A body cut mid-character by the executor is still text.
            Err(e) if truncated && e.error_len().is_none() => {
                BodyDetail::of_bytes(&bytes[..e.valid_up_to()], size, true)
            }
            Err(_) => {
                let end = bytes.len().min(MAX_DETAIL_BODY);
                BodyDetail::Base64 {
                    data: base64::engine::general_purpose::STANDARD.encode(&bytes[..end]),
                    size,
                    truncated: truncated || end < bytes.len(),
                }
            }
        }
    }

    /// Bytes this body takes in a stored run, for the run's body budget.
    pub fn stored_len(&self) -> usize {
        match self {
            BodyDetail::None | BodyDetail::Omitted { .. } => 0,
            BodyDetail::Text { text, .. } => text.len(),
            BodyDetail::Base64 { data, .. } => data.len(),
            BodyDetail::Form { fields } => fields.iter().map(|(k, v)| k.len() + v.len()).sum(),
            BodyDetail::Multipart { parts } => parts
                .iter()
                .map(|p| match p {
                    PartDetail::Text { name, value, .. } => name.len() + value.len(),
                    PartDetail::File {
                        name, file_name, ..
                    } => name.len() + file_name.len(),
                })
                .sum(),
        }
    }

    /// The body's full size in bytes, as far as it is known.
    pub fn size(&self) -> usize {
        match self {
            BodyDetail::Text { size, .. }
            | BodyDetail::Base64 { size, .. }
            | BodyDetail::Omitted { size, .. } => *size,
            other => other.stored_len(),
        }
    }
}
