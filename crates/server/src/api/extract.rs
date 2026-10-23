//! Extractors whose rejections use the API's JSON error shape instead of
//! axum's plain-text bodies.

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts};
use axum::http::StatusCode;

use crate::error::ApiError;

#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct ApiJson<T>(pub T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
pub struct ApiQuery<T>(pub T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct ApiPath<T>(pub T);

impl From<JsonRejection> for ApiError {
    fn from(r: JsonRejection) -> Self {
        let code = match r {
            JsonRejection::MissingJsonContentType(_) => "unsupported_media_type",
            _ if r.status() == StatusCode::PAYLOAD_TOO_LARGE => "payload_too_large",
            _ => "bad_request",
        };
        ApiError::new(r.status(), code, r.body_text())
    }
}

impl From<QueryRejection> for ApiError {
    fn from(r: QueryRejection) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, "bad_request", r.body_text())
    }
}

impl From<PathRejection> for ApiError {
    fn from(r: PathRejection) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, "bad_request", r.body_text())
    }
}
