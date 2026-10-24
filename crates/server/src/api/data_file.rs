//! `POST /collections/{id}/data-file/preview`: what a data file holds, for the
//! "Run with data…" dialog — the row count and column names, never a value.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use plover_core::data_file;

use super::dto::{DataFileDto, DataPreviewDto};
use super::extract::{ApiJson, ApiPath};
use super::open_collection;
use crate::error::ApiError;
use crate::state::AppState;

pub async fn preview(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<DataFileDto>,
) -> Result<Json<DataPreviewDto>, ApiError> {
    open_collection(&state, &id)?;
    let file = data_file::parse_file(body.format.into(), &body.text).map_err(|e| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_data",
            e.with_name("data file"),
        )
    })?;
    Ok(Json(DataPreviewDto {
        rows: u32::try_from(file.rows.len()).unwrap_or(u32::MAX),
        columns: file.columns,
    }))
}
