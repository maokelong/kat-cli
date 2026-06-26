#[allow(dead_code)]
mod support;

use kat_rs_cli::trace_runtime::{
    adapter::{DatasetAdapter, sqlite::SQLiteDatasetAdapter},
    pack::spec::{InputTables, TransformOutputSpec, TransformSafetySpec, TransformSpec},
    transform::payload::run_payload_extract_fields_transform,
};
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn payload_transform_uses_pack_extractor_marker_and_fields() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute(
        "CREATE TABLE events (id INTEGER, marker_name TEXT, marker_payload TEXT)",
        [],
    )
    .expect("events table");
    conn.execute(
        "INSERT INTO events VALUES
            (1, 'target_marker', 'start=10,end=25'),
            (2, 'other_marker', 'start=30,end=45')",
        [],
    )
    .expect("events rows");
    drop(conn);

    let pack = support::extractor_pack(
        "window_fields",
        json!({
            "source_table": "events",
            "payload_column": "marker_payload",
            "marker": {
                "column": "marker_name",
                "equals": "target_marker"
            },
            "fields": {
                "start_ts": "start",
                "end_ts": "end"
            }
        }),
    );
    let transform = support::payload_transform(vec!["events"], vec!["events"]);
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    run_payload_extract_fields_transform(&mut adapter, &pack, &transform)
        .expect("payload transform runs");

    let rows = adapter
        .query_json(&format!(
            "SELECT start_ts, end_ts FROM {} ORDER BY start_ts",
            transform.output.table
        ))
        .expect("query derived");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["start_ts"], 10);
    assert_eq!(rows[0]["end_ts"], 25);
}

#[test]
fn payload_transform_rejects_source_table_outside_declared_inputs() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute("CREATE TABLE events (id INTEGER)", [])
        .expect("events table");
    conn.execute(
        "CREATE TABLE secret_events (id INTEGER, marker_payload TEXT)",
        [],
    )
    .expect("secret events table");
    drop(conn);

    let pack = support::extractor_pack(
        "window_fields",
        json!({
            "source_table": "secret_events",
            "payload_column": "marker_payload",
            "fields": {
                "start_ts": "start"
            }
        }),
    );
    let transform = test_transform(
        InputTables::List(vec!["events".to_string()]),
        Default::default(),
    );
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_payload_extract_fields_transform(&mut adapter, &pack, &transform)
        .expect_err("source table outside inputs is rejected");

    assert!(
        error.to_string().contains("outside transform inputs"),
        "error: {error:#}"
    );
    assert!(
        !adapter
            .table_exists(&transform.output.table)
            .expect("output table check")
    );
}

#[test]
fn payload_transform_rejects_source_table_outside_allowed_tables() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute(
        "CREATE TABLE secret_events (id INTEGER, marker_payload TEXT)",
        [],
    )
    .expect("secret events table");
    drop(conn);

    let pack = support::extractor_pack(
        "window_fields",
        json!({
            "source_table": "secret_events",
            "payload_column": "marker_payload",
            "fields": {
                "start_ts": "start"
            }
        }),
    );
    let transform = test_transform(
        InputTables::List(vec!["secret_events".to_string()]),
        TransformSafetySpec {
            allowed_tables: vec!["events".to_string()],
        },
    );
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_payload_extract_fields_transform(&mut adapter, &pack, &transform)
        .expect_err("source table outside allowed tables is rejected");

    assert!(
        error.to_string().contains("outside safety.allowedTables"),
        "error: {error:#}"
    );
    assert!(
        !adapter
            .table_exists(&transform.output.table)
            .expect("output table check")
    );
}

#[test]
fn payload_transform_rejects_missing_allowed_tables() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute("CREATE TABLE events (id INTEGER, marker_payload TEXT)", [])
        .expect("events table");
    drop(conn);

    let pack = support::extractor_pack("window_fields", support::basic_payload_extractor("events"));
    let transform = test_transform(
        InputTables::List(vec!["events".to_string()]),
        Default::default(),
    );
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_payload_extract_fields_transform(&mut adapter, &pack, &transform)
        .expect_err("missing allowedTables is rejected");

    assert!(
        error.to_string().contains("safety.allowedTables"),
        "error: {error:#}"
    );
}

#[test]
fn payload_transform_rejects_empty_allowed_tables() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute("CREATE TABLE events (id INTEGER, marker_payload TEXT)", [])
        .expect("events table");
    drop(conn);

    let pack = support::extractor_pack("window_fields", support::basic_payload_extractor("events"));
    let transform = test_transform(
        InputTables::List(vec!["events".to_string()]),
        TransformSafetySpec {
            allowed_tables: Vec::new(),
        },
    );
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_payload_extract_fields_transform(&mut adapter, &pack, &transform)
        .expect_err("empty allowedTables is rejected");

    assert!(
        error.to_string().contains("safety.allowedTables"),
        "error: {error:#}"
    );
}

#[test]
fn payload_transform_rejects_empty_fields() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute("CREATE TABLE events (id INTEGER, marker_payload TEXT)", [])
        .expect("events table");
    drop(conn);

    let pack = support::extractor_pack(
        "window_fields",
        json!({
            "source_table": "events",
            "payload_column": "marker_payload",
            "fields": {}
        }),
    );
    let transform = test_transform(
        InputTables::List(vec!["events".to_string()]),
        TransformSafetySpec {
            allowed_tables: vec!["events".to_string()],
        },
    );
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_payload_extract_fields_transform(&mut adapter, &pack, &transform)
        .expect_err("empty fields are rejected");

    assert!(
        error.to_string().contains("has no fields"),
        "error: {error:#}"
    );
    assert!(
        !adapter
            .table_exists(&transform.output.table)
            .expect("output table check")
    );
}

fn test_transform(inputs: InputTables, safety: TransformSafetySpec) -> TransformSpec {
    TransformSpec {
        id: "window_fields".to_string(),
        kind: "payload.extract_fields".to_string(),
        inputs,
        sql: None,
        params: Default::default(),
        bind: Default::default(),
        where_: Default::default(),
        output: TransformOutputSpec {
            table: "derived_windows".to_string(),
            schema: "window.fields.v1".to_string(),
            semantic: None,
        },
        materialize: None,
        safety,
    }
}
