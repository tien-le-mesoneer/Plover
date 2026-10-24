//! curl paste and "Copy as cURL".

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use plover_core::curl::parse_curl;
use plover_core::request::HttpRequest;
use plover_core::runner::{SendInput, copy_as_curl};

use super::dto::{CopyCurlBody, CurlParseBody, CurlParsedDto, CurlTextDto, HttpRequestDto};
use super::extract::{ApiJson, ApiPath};
use super::open_collection;
use crate::error::ApiError;
use crate::state::AppState;

pub async fn parse(ApiJson(body): ApiJson<CurlParseBody>) -> Result<Json<CurlParsedDto>, ApiError> {
    let parsed = parse_curl(&body.text)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "invalid_curl", e.to_string()))?;
    Ok(Json(CurlParsedDto {
        request: HttpRequestDto::from(&parsed.request),
        notices: parsed.notices.iter().map(Into::into).collect(),
    }))
}

pub async fn copy(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<CopyCurlBody>,
) -> Result<Json<CurlTextDto>, ApiError> {
    let open = open_collection(&state, &id)?;
    if body.item.is_none() && body.draft.is_none() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_item",
            "a scratch request needs a draft",
        ));
    }
    let draft = body.draft.map(HttpRequest::from);
    let copied = copy_as_curl(
        &open,
        SendInput {
            item_id: body.item.as_deref(),
            draft: draft.as_ref(),
            env: body.env.as_deref(),
        },
        body.resolve,
    )?;
    Ok(Json(CurlTextDto {
        text: copied.text,
        contains_secret: copied.contains_secret,
        omitted: copied.omitted,
    }))
}
