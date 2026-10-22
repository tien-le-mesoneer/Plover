use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// Every API error has the body `{"error": "<snake_case_code>", "message": "..."}`.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    /// Extra fields merged into the body (e.g. `current` for conflicts).
    pub extra: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct Body<'a> {
    error: &'a str,
    message: &'a str,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    extra: Option<&'a serde_json::Value>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError {
            status,
            code,
            message: message.into(),
            extra: None,
        }
    }

    pub fn not_found(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "not_found", message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Body {
            error: self.code,
            message: &self.message,
            extra: self.extra.as_ref(),
        };
        (self.status, Json(body)).into_response()
    }
}
