//! CSV and JSON data files for data-driven runs: one row per iteration.
//!
//! Matches Bruno CLI 4.3.0 (`utils/filesystem.js` `parseCSV`, csv-parse with
//! `columns: true, skip_empty_lines: true, trim: true`), except that every way of
//! getting zero rows is an error here instead of a silent empty run.

use std::path::Path;

use serde_json::Value;

use crate::credentials::is_credential_field;
use crate::vars::ValueMap;

/// Largest data file Plover reads.
pub const MAX_DATA_FILE: usize = 16 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataFormat {
    Csv,
    Json,
}

impl DataFormat {
    /// `.csv` or `.json`, ignoring case.
    pub fn from_path(path: &Path) -> Option<DataFormat> {
        let ext = path.extension()?.to_str()?;
        if ext.eq_ignore_ascii_case("csv") {
            Some(DataFormat::Csv)
        } else if ext.eq_ignore_ascii_case("json") {
            Some(DataFormat::Json)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DataFileError {
    #[error("file is larger than 16 MiB ({size} bytes)")]
    TooLarge { size: usize },
    #[error("file has no rows")]
    Empty,
    #[error("{message}")]
    Csv { line: u64, message: String },
    #[error("invalid JSON: {0}")]
    Json(String),
    #[error("the top level must be an array of objects")]
    NotArray,
    #[error("element {index} is not an object")]
    NotObject { index: usize },
}

impl DataFileError {
    /// The message with the file name in front, and the line for CSV errors:
    /// `data.csv:7: expected 4 fields, found 3`.
    pub fn with_name(&self, name: &str) -> String {
        match self {
            DataFileError::Csv { line, message } => format!("{name}:{line}: {message}"),
            e => format!("{name}: {e}"),
        }
    }
}

/// Rows plus the column names, in file order for CSV (rows themselves are sorted maps).
#[derive(Debug, Clone, PartialEq)]
pub struct DataFile {
    pub columns: Vec<String>,
    pub rows: Vec<ValueMap>,
}

pub fn parse(format: DataFormat, text: &str) -> Result<Vec<ValueMap>, DataFileError> {
    parse_file(format, text).map(|f| f.rows)
}

pub fn parse_file(format: DataFormat, text: &str) -> Result<DataFile, DataFileError> {
    if text.len() > MAX_DATA_FILE {
        return Err(DataFileError::TooLarge { size: text.len() });
    }
    match format {
        DataFormat::Csv => parse_csv_file(text),
        DataFormat::Json => {
            let rows = parse_json(text)?;
            Ok(DataFile {
                columns: columns(&rows),
                rows,
            })
        }
    }
}

/// The first record holds the column names; values are trimmed strings. Empty lines are
/// skipped, a record with a different field count is an error, and a repeated column
/// name keeps its last value (as csv-parse does).
pub fn parse_csv(text: &str) -> Result<Vec<ValueMap>, DataFileError> {
    parse_csv_file(text).map(|f| f.rows)
}

fn parse_csv_file(text: &str) -> Result<DataFile, DataFileError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .flexible(false)
        .from_reader(text.as_bytes());
    let headers = reader.headers().map_err(csv_error)?.clone();
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(csv_error)?;
        let row = headers
            .iter()
            .zip(record.iter())
            .map(|(name, value)| (name.to_owned(), Value::String(value.to_owned())))
            .collect();
        rows.push(row);
    }
    if rows.is_empty() {
        return Err(DataFileError::Empty);
    }
    let mut columns: Vec<String> = Vec::new();
    for name in headers.iter() {
        if !columns.iter().any(|c| c == name) {
            columns.push(name.to_owned());
        }
    }
    Ok(DataFile { columns, rows })
}

fn csv_error(e: csv::Error) -> DataFileError {
    let line = e.position().map(|p| p.line()).unwrap_or(0);
    let message = match e.kind() {
        csv::ErrorKind::UnequalLengths {
            expected_len, len, ..
        } => format!("expected {expected_len} fields, found {len}"),
        csv::ErrorKind::Utf8 { .. } => "not valid UTF-8".to_owned(),
        _ => e.to_string(),
    };
    DataFileError::Csv { line, message }
}

/// The top level is an array, and each element is one row; values keep their JSON type.
pub fn parse_json(text: &str) -> Result<Vec<ValueMap>, DataFileError> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| DataFileError::Json(e.to_string()))?;
    let Value::Array(items) = value else {
        return Err(DataFileError::NotArray);
    };
    if items.is_empty() {
        return Err(DataFileError::Empty);
    }
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| match item {
            Value::Object(map) => Ok(map.into_iter().collect()),
            _ => Err(DataFileError::NotObject { index }),
        })
        .collect()
}

/// Column names in first-seen order across rows. Keys within one row come sorted
/// (`ValueMap` is a sorted map); `parse_file` keeps the CSV header order instead.
pub fn columns(rows: &[ValueMap]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        for key in row.keys() {
            if seen.insert(key.clone()) {
                out.push(key.clone());
            }
        }
    }
    out
}

/// String values of credential-named keys at any depth in every row, for the masker.
pub fn credential_values(rows: &[ValueMap]) -> Vec<String> {
    let mut out = Vec::new();
    for row in rows {
        for (key, value) in row {
            collect(key, value, &mut out);
        }
    }
    out.sort();
    out.dedup();
    out
}

fn collect(key: &str, value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) if is_credential_field(key) => out.push(s.clone()),
        Value::Object(map) => {
            for (k, v) in map {
                collect(k, v, out);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect(key, v, out);
            }
        }
        _ => {}
    }
}
