use plover_core::data_file::{
    DataFileError, DataFormat, MAX_DATA_FILE, columns, credential_values, parse, parse_csv,
    parse_file, parse_json,
};
use plover_core::vars::ValueMap;
use serde_json::{Value, json};
use std::path::Path;

fn row(v: Value) -> ValueMap {
    v.as_object().unwrap().clone().into_iter().collect()
}

#[test]
fn csv_basic() {
    let rows = parse_csv("user,pass\nalice,p1\nbob,p2\n").unwrap();
    assert_eq!(
        rows,
        vec![
            row(json!({"user":"alice","pass":"p1"})),
            row(json!({"user":"bob","pass":"p2"}))
        ]
    );
}

#[test]
fn csv_bom_crlf_quotes_blank_lines() {
    let crlf = "\u{feff}a,b\r\n\"x,1\",\"line1\nline2\"\r\n\r\n y , z \r\n";
    let want = vec![
        row(json!({"a":"x,1","b":"line1\nline2"})),
        row(json!({"a":"y","b":"z"})),
    ];
    assert_eq!(parse_csv(crlf).unwrap(), want);
    let lf = "a,b\n\"x,1\",\"line1\nline2\"\n\n y , z \n";
    assert_eq!(parse_csv(lf).unwrap(), want);
}

#[test]
fn csv_row_of_empty_fields_is_kept_like_csv_parse() {
    let rows = parse_csv("a,b\n,\n1,2\n").unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], row(json!({"a":"","b":""})));
}

#[test]
fn csv_wrong_field_count_names_line() {
    let text = "a,b,c,d\n1,2,3,4\n1,2,3,4\n1,2,3,4\n1,2,3,4\n1,2,3,4\n1,2,3\n";
    let err = parse_csv(text).unwrap_err();
    assert!(matches!(err, DataFileError::Csv { line: 7, .. }), "{err:?}");
    let msg = err.with_name("data.csv");
    assert!(msg.starts_with("data.csv:7: "), "{msg}");
    assert!(msg.contains("expected 4 fields, found 3"), "{msg}");
}

#[test]
fn csv_header_only_is_empty() {
    assert_eq!(parse_csv("a,b\n").unwrap_err(), DataFileError::Empty);
    assert_eq!(parse_csv("").unwrap_err(), DataFileError::Empty);
}

#[test]
fn csv_duplicate_columns_last_wins() {
    let f = parse_file(DataFormat::Csv, "a,a,b\n1,2,3\n").unwrap();
    assert_eq!(f.rows, vec![row(json!({"a":"2","b":"3"}))]);
    assert_eq!(f.columns, vec!["a", "b"]);
}

#[test]
fn csv_columns_keep_header_order() {
    let f = parse_file(DataFormat::Csv, "user,password,id\nu,p,1\n").unwrap();
    assert_eq!(f.columns, vec!["user", "password", "id"]);
}

#[test]
fn json_rows_keep_types() {
    let rows = parse_json(r#"[{"n":3,"b":true,"z":null,"o":{"a":1}}]"#).unwrap();
    assert_eq!(rows[0]["n"], json!(3));
    assert_eq!(rows[0]["b"], json!(true));
    assert_eq!(rows[0]["z"], Value::Null);
    assert_eq!(rows[0]["o"], json!({"a":1}));
}

#[test]
fn json_shape_errors() {
    assert_eq!(parse_json("{}").unwrap_err(), DataFileError::NotArray);
    assert_eq!(
        parse_json("[1]").unwrap_err(),
        DataFileError::NotObject { index: 0 }
    );
    assert_eq!(parse_json("[]").unwrap_err(), DataFileError::Empty);
    assert!(matches!(
        parse_json("[").unwrap_err(),
        DataFileError::Json(_)
    ));
}

#[test]
fn too_large() {
    let text = "a".repeat(MAX_DATA_FILE + 1);
    assert!(matches!(
        parse(DataFormat::Csv, &text),
        Err(DataFileError::TooLarge { .. })
    ));
    assert!(matches!(
        parse(DataFormat::Json, &text),
        Err(DataFileError::TooLarge { .. })
    ));
}

#[test]
fn columns_first_seen_order() {
    let rows = vec![row(json!({"b":1})), row(json!({"a":1,"c":2}))];
    assert_eq!(columns(&rows), vec!["b", "a", "c"]);
}

#[test]
fn credential_values_by_name() {
    let rows = vec![
        row(json!({"password":"pw-1234567","username":"alice"})),
        row(json!({"Client_Secret":"cs-1234567","newPassword":"np-1234567"})),
        row(json!({"auth":{"token":"t-1234567"},"token":7})),
    ];
    assert_eq!(
        credential_values(&rows),
        vec!["cs-1234567", "pw-1234567", "t-1234567"]
    );
}

#[test]
fn format_from_path() {
    assert_eq!(
        DataFormat::from_path(Path::new("x/ROWS.CSV")),
        Some(DataFormat::Csv)
    );
    assert_eq!(
        DataFormat::from_path(Path::new("rows.json")),
        Some(DataFormat::Json)
    );
    assert_eq!(DataFormat::from_path(Path::new("rows.txt")), None);
}
