//! Names that carry credentials, shared by the HAR and curl import (which moves their
//! values into secret variables) and by data-driven runs (which mask them).
//!
//! The lists are fixed and shown to the user, so what gets treated as a credential is
//! predictable. Matching is ASCII case-insensitive and exact: `newPassword` and
//! `password2` are not credential fields.

use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

/// Body fields (form, multipart text and JSON keys at any depth) and data-file columns.
pub const CREDENTIAL_FIELDS: [&str; 12] = [
    "password",
    "passwd",
    "client_secret",
    "secret",
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "api_key",
    "apikey",
    "assertion",
    "client_assertion",
];

/// Request headers that carry a token besides `Authorization` and `Cookie`.
pub const TOKEN_HEADERS: [&str; 5] = [
    "x-api-key",
    "api-key",
    "x-auth-token",
    "x-csrf-token",
    "x-xsrf-token",
];

/// URL query parameters that carry a token.
pub const TOKEN_QUERY_PARAMS: [&str; 5] = ["access_token", "token", "api_key", "apikey", "key"];

pub fn is_credential_field(name: &str) -> bool {
    CREDENTIAL_FIELDS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(name))
}

pub fn is_token_header(name: &str) -> bool {
    TOKEN_HEADERS.iter().any(|h| h.eq_ignore_ascii_case(name))
}

pub fn is_token_query_param(name: &str) -> bool {
    TOKEN_QUERY_PARAMS
        .iter()
        .any(|q| q.eq_ignore_ascii_case(name))
}

/// A value that looks like a JWT: `eyJ…` base64url header, a dot, and more.
pub fn looks_like_jwt(text: &str) -> bool {
    text.match_indices("eyJ")
        .any(|(i, _)| jwt_header_end(text, i).is_some())
}

fn is_base64url(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')
}

/// End of the `eyJ…` header starting at `start` (the index of its dot), when it is at
/// least 10 base64url characters long and followed by a dot.
fn jwt_header_end(text: &str, start: usize) -> Option<usize> {
    let rest = &text.as_bytes()[start..];
    let len = rest.iter().take_while(|b| is_base64url(**b)).count();
    (len >= 10 && rest.get(len) == Some(&b'.')).then_some(start + len)
}

/// Byte ranges of JWT-shaped tokens: an `eyJ…` header (as in `looks_like_jwt`) that does
/// not continue a longer base64url run, a dot, a non-empty payload, and optionally a dot
/// and a signature (which may be empty).
pub fn jwt_spans(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let run = |from: usize| {
        from + bytes[from..]
            .iter()
            .take_while(|b| is_base64url(**b))
            .count()
    };
    let mut out: Vec<Range<usize>> = Vec::new();
    for (start, _) in text.match_indices("eyJ") {
        if out.last().is_some_and(|r| start < r.end)
            || (start > 0 && is_base64url(bytes[start - 1]))
        {
            continue;
        }
        let Some(dot) = jwt_header_end(text, start) else {
            continue;
        };
        let payload_end = run(dot + 1);
        if payload_end == dot + 1 {
            continue;
        }
        let end = if bytes.get(payload_end) == Some(&b'.') {
            run(payload_end + 1)
        } else {
            payload_end
        };
        out.push(start..end);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    JsonString,
    JsonNumber,
    XmlText,
    XmlAttr,
    Jwt,
}

/// A credential value inside a body's text: the raw text as written (JSON escapes and
/// XML entities are not decoded; a JSON string span excludes the quotes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialSpan {
    pub range: Range<usize>,
    /// The JSON key or local XML element/attribute name as written; `None` for a JWT.
    pub name: Option<String>,
    pub kind: SpanKind,
}

/// Credential value spans in a body: credential-named JSON values (when the text parses
/// as JSON), credential-named XML elements and attributes (when its first non-space
/// character is `<`), plus JWT spans anywhere not already inside a named span. Sorted,
/// non-overlapping, never empty.
pub fn body_credential_spans(text: &str) -> Vec<CredentialSpan> {
    let mut spans = if serde_json::from_str::<serde_json::Value>(text).is_ok() {
        json_spans(text)
    } else if text.trim_start().starts_with('<') {
        xml_spans(text)
    } else {
        Vec::new()
    };
    for range in jwt_spans(text) {
        if !spans
            .iter()
            .any(|s| s.range.start < range.end && range.start < s.range.end)
        {
            spans.push(CredentialSpan {
                range,
                name: None,
                kind: SpanKind::Jwt,
            });
        }
    }
    spans.retain(|s| !s.range.is_empty());
    spans.sort_by_key(|s| s.range.start);
    spans
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum JsonToken {
    /// The raw text between the quotes.
    Str(usize, usize),
    Num(usize, usize),
    Colon,
    Other,
}

/// Tokens of a JSON text that already parsed, so the scan never sees malformed input.
fn json_tokens(text: &str) -> Vec<JsonToken> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j] != b'"' {
                    j += if bytes[j] == b'\\' { 2 } else { 1 };
                }
                tokens.push(JsonToken::Str(start, j));
                i = j + 1;
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                while i < bytes.len()
                    && matches!(bytes[i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    i += 1;
                }
                tokens.push(JsonToken::Num(start, i));
            }
            b':' => {
                tokens.push(JsonToken::Colon);
                i += 1;
            }
            b if b.is_ascii_whitespace() => i += 1,
            _ => {
                tokens.push(JsonToken::Other);
                i += 1;
            }
        }
    }
    tokens
}

fn json_spans(text: &str) -> Vec<CredentialSpan> {
    let tokens = json_tokens(text);
    let mut out = Vec::new();
    for w in tokens.windows(3) {
        let [JsonToken::Str(ks, ke), JsonToken::Colon, value] = w else {
            continue;
        };
        let Ok(key) = serde_json::from_str::<String>(&text[ks - 1..ke + 1]) else {
            continue;
        };
        if !is_credential_field(&key) {
            continue;
        }
        let (range, kind) = match *value {
            JsonToken::Str(s, e) => (s..e, SpanKind::JsonString),
            JsonToken::Num(s, e) => (s..e, SpanKind::JsonNumber),
            _ => continue,
        };
        out.push(CredentialSpan {
            range,
            name: Some(key),
            kind,
        });
    }
    out
}

fn xml_spans(text: &str) -> Vec<CredentialSpan> {
    static OPEN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<(?:[\w.-]+:)?([\w.-]+)(\s[^<>]*)?>").expect("valid regex"));
    static CLOSE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^</(?:[\w.-]+:)?([\w.-]+)\s*>").expect("valid regex"));
    static ATTR: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?:[\w.-]+:)?([\w.-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("valid regex")
    });
    let mut out = Vec::new();
    for tag in OPEN.captures_iter(text) {
        let whole = tag.get(0).expect("match");
        let local = &tag[1];
        if let Some(attrs) = tag.get(2) {
            for a in ATTR.captures_iter(attrs.as_str()) {
                let Some(value) = a.get(2).or_else(|| a.get(3)) else {
                    continue;
                };
                if is_credential_field(&a[1]) && !value.is_empty() {
                    let base = attrs.start();
                    out.push(CredentialSpan {
                        range: base + value.start()..base + value.end(),
                        name: Some(a[1].to_owned()),
                        kind: SpanKind::XmlAttr,
                    });
                }
            }
        }
        if whole.as_str().ends_with("/>") || !is_credential_field(local) {
            continue;
        }
        let after = &text[whole.end()..];
        let (content, rest_at) = if let Some(cdata) = after.strip_prefix("<![CDATA[") {
            let Some(end) = cdata.find("]]>") else {
                continue;
            };
            let start = whole.end() + "<![CDATA[".len();
            (start..start + end, start + end + "]]>".len())
        } else {
            let end = after.find('<').unwrap_or(after.len());
            (whole.end()..whole.end() + end, whole.end() + end)
        };
        let closes = CLOSE
            .captures(&text[rest_at..])
            .is_some_and(|c| &c[1] == local);
        if closes && !content.is_empty() {
            out.push(CredentialSpan {
                range: content,
                name: Some(local.to_owned()),
                kind: SpanKind::XmlText,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_match_exactly_ignoring_case() {
        assert!(is_credential_field("Client_Secret"));
        assert!(is_credential_field("PASSWORD"));
        assert!(!is_credential_field("newPassword"));
        assert!(!is_credential_field("password2"));
        assert!(is_token_header("X-CSRF-Token"));
        assert!(is_token_query_param("access_token"));
        assert!(!is_token_query_param("tokens"));
    }

    #[test]
    fn jwt_detection() {
        assert!(looks_like_jwt(
            "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig"
        ));
        assert!(!looks_like_jwt("eyJ"));
        assert!(!looks_like_jwt("{\"eyJob\": 1}"));
    }
}
