use kat_rs_cli::trace_runtime::{
    adapter::{DatasetAdapter, sqlite::SQLiteDatasetAdapter},
    primitives::payload::{PayloadExtractorSpec, PayloadMarkerFilter, run_payload_extract_fields},
};
use rusqlite::Connection;
use tempfile::tempdir;

#[test]
fn payload_extract_fields_uses_configured_payload_keys() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute(
        "CREATE TABLE callstack (id INTEGER, marker_payload TEXT)",
        [],
    )
    .expect("callstack table");
    conn.execute(
        "INSERT INTO callstack VALUES (1, 'start=10,end=25'), (2, 'start=30,end=40')",
        [],
    )
    .expect("callstack rows");
    drop(conn);

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let spec = PayloadExtractorSpec {
        source_table: "callstack".to_string(),
        output_table: "first_draw_window".to_string(),
        payload_column: "marker_payload".to_string(),
        marker: None,
        fields: vec![
            ("start_ts".to_string(), "start".to_string()),
            ("end_ts".to_string(), "end".to_string()),
        ],
    };

    run_payload_extract_fields(&mut adapter, &spec).expect("extract runs");

    let rows = DatasetAdapter::query_json(
        &mut adapter,
        "SELECT start_ts, end_ts FROM first_draw_window ORDER BY start_ts",
    )
    .expect("query extracted");
    assert_eq!(rows[0]["start_ts"], 10);
    assert_eq!(rows[0]["end_ts"], 25);
}

#[test]
fn payload_extract_fields_returns_null_for_missing_first_key() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute(
        "CREATE TABLE callstack (id INTEGER, marker_payload TEXT)",
        [],
    )
    .expect("callstack table");
    conn.execute("INSERT INTO callstack VALUES (1, 'end=25')", [])
        .expect("callstack rows");
    drop(conn);

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let spec = PayloadExtractorSpec {
        source_table: "callstack".to_string(),
        output_table: "window_fields".to_string(),
        payload_column: "marker_payload".to_string(),
        marker: None,
        fields: vec![
            ("start_ts".to_string(), "start".to_string()),
            ("end_ts".to_string(), "end".to_string()),
        ],
    };

    run_payload_extract_fields(&mut adapter, &spec).expect("extract runs");

    let rows =
        DatasetAdapter::query_json(&mut adapter, "SELECT start_ts, end_ts FROM window_fields")
            .expect("query extracted");
    assert!(rows[0]["start_ts"].is_null());
    assert_eq!(rows[0]["end_ts"], 25);
}

#[test]
fn payload_extract_fields_returns_null_for_missing_second_key() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute(
        "CREATE TABLE callstack (id INTEGER, marker_payload TEXT)",
        [],
    )
    .expect("callstack table");
    conn.execute("INSERT INTO callstack VALUES (1, 'start=10')", [])
        .expect("callstack rows");
    drop(conn);

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let spec = PayloadExtractorSpec {
        source_table: "callstack".to_string(),
        output_table: "window_fields".to_string(),
        payload_column: "marker_payload".to_string(),
        marker: None,
        fields: vec![
            ("start_ts".to_string(), "start".to_string()),
            ("end_ts".to_string(), "end".to_string()),
        ],
    };

    run_payload_extract_fields(&mut adapter, &spec).expect("extract runs");

    let rows =
        DatasetAdapter::query_json(&mut adapter, "SELECT start_ts, end_ts FROM window_fields")
            .expect("query extracted");
    assert_eq!(rows[0]["start_ts"], 10);
    assert!(rows[0]["end_ts"].is_null());
}

#[test]
fn payload_extract_fields_extracts_final_field_without_trailing_comma() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute(
        "CREATE TABLE callstack (id INTEGER, marker_payload TEXT)",
        [],
    )
    .expect("callstack table");
    conn.execute("INSERT INTO callstack VALUES (1, 'alpha=7,beta=11')", [])
        .expect("callstack rows");
    drop(conn);

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let spec = PayloadExtractorSpec {
        source_table: "callstack".to_string(),
        output_table: "payload_fields".to_string(),
        payload_column: "marker_payload".to_string(),
        marker: None,
        fields: vec![("beta_value".to_string(), "beta".to_string())],
    };

    run_payload_extract_fields(&mut adapter, &spec).expect("extract runs");

    let rows = DatasetAdapter::query_json(&mut adapter, "SELECT beta_value FROM payload_fields")
        .expect("query extracted");
    assert_eq!(rows[0]["beta_value"], 11);
}

#[test]
fn payload_extract_fields_filters_by_marker_when_configured() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute(
        "CREATE TABLE callstack (id INTEGER, marker_name TEXT, marker_payload TEXT)",
        [],
    )
    .expect("callstack table");
    conn.execute(
        "INSERT INTO callstack VALUES
            (1, 'target_marker''s', 'start=10,end=25'),
            (2, 'other_marker', 'start=30,end=40'),
            (3, 'target_marker''s', NULL)",
        [],
    )
    .expect("callstack rows");
    drop(conn);

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let spec = PayloadExtractorSpec {
        source_table: "callstack".to_string(),
        output_table: "filtered_payload_fields".to_string(),
        payload_column: "marker_payload".to_string(),
        marker: Some(PayloadMarkerFilter {
            column: "marker_name".to_string(),
            equals: "target_marker's".to_string(),
        }),
        fields: vec![
            ("start_ts".to_string(), "start".to_string()),
            ("end_ts".to_string(), "end".to_string()),
        ],
    };

    run_payload_extract_fields(&mut adapter, &spec).expect("extract runs");

    let rows = DatasetAdapter::query_json(
        &mut adapter,
        "SELECT start_ts, end_ts FROM filtered_payload_fields ORDER BY start_ts",
    )
    .expect("query extracted");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["start_ts"], 10);
    assert_eq!(rows[0]["end_ts"], 25);
}
