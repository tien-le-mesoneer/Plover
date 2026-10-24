//! Names that carry credentials, shared by the HAR and curl import (which moves their
//! values into secret variables) and by data-driven runs (which mask them).
//!
//! The lists are fixed and shown to the user, so what gets treated as a credential is
//! predictable. Matching is ASCII case-insensitive and exact: `newPassword` and
//! `password2` are not credential fields.

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
    text.match_indices("eyJ").any(|(i, _)| {
        let rest = &text[i..];
        let header_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            .count();
        header_len >= 10 && rest.as_bytes().get(header_len) == Some(&b'.')
    })
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
