use std::{fs, path::Path};

use kat_rs_cli::trace_runtime::{
    adapter::{DatasetAdapter, sqlite::SQLiteDatasetAdapter},
    pack::{load_pack, spec::TransformSpec},
    transform::{
        derived_runner::DerivedRunner, marker::run_marker_extract_bracket_fields_transform,
    },
};
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

fn create_raw_db(path: &Path, ddl: &str, inserts: &[&str]) {
    let conn = Connection::open(path).expect("raw sqlite db opens");
    conn.execute_batch(ddl).expect("raw schema is created");
    for insert in inserts {
        conn.execute(insert, []).expect("raw row is inserted");
    }
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent dir is created");
    }
    fs::write(path, content).expect("file is written");
}

#[test]
fn sqlite_adapter_reports_table_columns() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    create_raw_db(
        &raw_db,
        "CREATE TABLE thread (itid INTEGER, thread_name TEXT);",
        &["INSERT INTO thread (itid, thread_name) VALUES (7, 'RenderThread')"],
    );

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter opens");
    let columns = adapter
        .table_columns("thread")
        .expect("thread columns load");
    let names = columns
        .iter()
        .map(|column| column.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["itid", "thread_name"]);
}

#[test]
fn transform_spec_rejects_sql_view_marker_fields() {
    let yaml = r#"
id: bad_sql
kind: sql.view
inputs: [thread]
sql: queries/thread.sql
source:
  table: callstack
  column: name
  contains: firstDrawFrame
output:
  table: thread_view
  schema: thread.view.v1
"#;

    let error = serde_yaml::from_str::<TransformSpec>(yaml)
        .expect_err("sql.view rejects marker-only source fields");

    assert!(
        error.to_string().contains("source"),
        "unexpected error: {error}"
    );
}

#[test]
fn transform_spec_loads_marker_extract_config() {
    let yaml = r#"
id: first_draw_window
kind: marker.extract_bracket_fields
inputs: [callstack, thread, process]
source:
  table: callstack
  column: name
  contains: "${params.marker}"
fields:
  start_ts: layoutMeasureDurationStartTimestamp
  end_ts: layoutMeasureDurationEndTimestamp
  vsync_id: vsyncID
filters:
  process_name: "${params.target_process}"
output:
  table: first_draw_window
  schema: marker.first_draw_window.v1
safety:
  allowedTables: [callstack, thread, process]
"#;

    let spec = serde_yaml::from_str::<TransformSpec>(yaml).expect("marker spec loads");

    assert_eq!(spec.id(), "first_draw_window");
    assert_eq!(spec.kind(), "marker.extract_bracket_fields");
    assert_eq!(spec.output().table, "first_draw_window");
    assert!(spec.uses_state_template() == false);
}

#[test]
fn marker_extract_outputs_all_matching_markers() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    create_raw_db(
        &raw_db,
        "
        CREATE TABLE process (ipid INTEGER, name TEXT);
        CREATE TABLE thread (itid INTEGER, tid INTEGER, ipid INTEGER, name TEXT);
        CREATE TABLE callstack (id INTEGER, parent_id INTEGER, callid INTEGER, name TEXT);
        ",
        &[
            "INSERT INTO process (ipid, name) VALUES (1, '.tencent.wechat')",
            "INSERT INTO thread (itid, tid, ipid, name) VALUES (10, 100, 1, 'main')",
            "INSERT INTO callstack (id, parent_id, callid, name) VALUES (1, NULL, 10, 'H:UIVsyncTask[vsyncID:1][layoutMeasureDurationStartTimestamp:100][layoutMeasureDurationEndTimestamp:200][firstDrawFrame:1]|M')",
            "INSERT INTO callstack (id, parent_id, callid, name) VALUES (2, NULL, 10, 'H:UIVsyncTask[vsyncID:2][layoutMeasureDurationStartTimestamp:300][layoutMeasureDurationEndTimestamp:450][firstDrawFrame:1]|M')",
            "INSERT INTO callstack (id, parent_id, callid, name) VALUES (3, NULL, 10, 'H:UIVsyncTask[vsyncID:3][layoutMeasureDurationStartTimestamp:300][layoutMeasureDurationEndTimestamp:400][firstDrawFrame:1]|M')",
        ],
    );
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter opens");
    let transform = serde_yaml::from_str::<TransformSpec>(
        r#"
id: first_draw_window
kind: marker.extract_bracket_fields
inputs: [callstack, thread, process]
source:
  table: callstack
  column: name
  contains: "${params.marker}"
fields:
  start_ts: layoutMeasureDurationStartTimestamp
  end_ts: layoutMeasureDurationEndTimestamp
  vsync_id: vsyncID
filters:
  process_name: "${params.target_process}"
output:
  table: first_draw_window
  schema: marker.first_draw_window.v1
safety:
  allowedTables: [callstack, thread, process]
"#,
    )
    .expect("marker transform spec loads");
    let TransformSpec::MarkerExtractBracketFields(spec) = transform else {
        panic!("expected marker transform spec");
    };

    run_marker_extract_bracket_fields_transform(
        &mut adapter,
        &spec,
        &json!({
            "marker": "firstDrawFrame:1",
            "target_process": ".tencent.wechat"
        }),
        &json!({}),
    )
    .expect("marker transform runs");
    drop(adapter);

    let conn = Connection::open(&scratch_db).expect("scratch db opens");
    let rows = conn
        .prepare(
            "SELECT callstack_id, vsync_id, start_ts, end_ts, dur_ns
             FROM first_draw_window
             ORDER BY rowid",
        )
        .expect("ordered marker query prepares")
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .expect("ordered marker query runs")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("ordered marker rows collect");

    assert_eq!(
        rows,
        vec![
            (1, 1, 100, 200, 100),
            (3, 3, 300, 400, 100),
            (2, 2, 300, 450, 150),
        ]
    );
}

#[test]
fn marker_extract_rejects_unsupported_filters() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    create_raw_db(
        &raw_db,
        "
        CREATE TABLE process (ipid INTEGER, name TEXT);
        CREATE TABLE thread (itid INTEGER, tid INTEGER, ipid INTEGER, name TEXT);
        CREATE TABLE callstack (id INTEGER, parent_id INTEGER, callid INTEGER, name TEXT);
        ",
        &[
            "INSERT INTO process (ipid, name) VALUES (1, '.tencent.wechat')",
            "INSERT INTO thread (itid, tid, ipid, name) VALUES (10, 100, 1, 'main')",
            "INSERT INTO callstack (id, parent_id, callid, name) VALUES (1, NULL, 10, 'H:UIVsyncTask[vsyncID:1][layoutMeasureDurationStartTimestamp:100][layoutMeasureDurationEndTimestamp:200][firstDrawFrame:1]|M')",
        ],
    );
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter opens");
    let transform = serde_yaml::from_str::<TransformSpec>(
        r#"
id: first_draw_window
kind: marker.extract_bracket_fields
inputs: [callstack, thread, process]
source:
  table: callstack
  column: name
  contains: "${params.marker}"
fields:
  start_ts: layoutMeasureDurationStartTimestamp
  end_ts: layoutMeasureDurationEndTimestamp
  vsync_id: vsyncID
filters:
  process_name: "${params.target_process}"
  thread_name: main
output:
  table: first_draw_window
  schema: marker.first_draw_window.v1
safety:
  allowedTables: [callstack, thread, process]
"#,
    )
    .expect("marker transform spec loads");
    let TransformSpec::MarkerExtractBracketFields(spec) = transform else {
        panic!("expected marker transform spec");
    };

    let error = run_marker_extract_bracket_fields_transform(
        &mut adapter,
        &spec,
        &json!({
            "marker": "firstDrawFrame:1",
            "target_process": ".tencent.wechat"
        }),
        &json!({}),
    )
    .expect_err("unsupported filters are rejected");
    let message = error.to_string();

    assert!(
        message.contains("unsupported filter keys"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains("thread_name"),
        "unexpected error: {message}"
    );
}

#[test]
fn derived_runner_records_materialization_metadata() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let pack_root = dir.path().join("pack");
    create_raw_db(
        &raw_db,
        "CREATE TABLE source_table (id INTEGER, label TEXT);",
        &["INSERT INTO source_table (id, label) VALUES (1, 'alpha')"],
    );
    write_file(
        &pack_root.join("pack.yaml"),
        r#"
id: metadata-pack
derived:
  - derived/source_view.yaml
queries:
  - queries/source_view.sql
"#,
    );
    write_file(
        &pack_root.join("derived/source_view.yaml"),
        r#"
id: source_view
kind: sql.view
inputs: [source_table]
sql: queries/source_view.sql
output:
  table: source_view
  schema: source.view.v1
  semantic: source_view
materialize: eager
safety:
  allowedTables: [source_table]
"#,
    );
    write_file(
        &pack_root.join("queries/source_view.sql"),
        "SELECT id, label FROM source_table",
    );

    let pack = load_pack(&pack_root).expect("pack loads");
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter opens");
    let mut runner = DerivedRunner::new(&pack).expect("runner is created");

    runner
        .ensure_table(&mut adapter, "source_view", &json!({}), &json!({}))
        .expect("derived table materializes");

    let metadata = runner
        .materialized_metadata("source_view")
        .expect("metadata is recorded");
    assert_eq!(metadata.pack_id, "metadata-pack");
    assert_eq!(metadata.transform_id, "source_view");
    assert_eq!(metadata.input_tables, vec!["source_table"]);
    assert_eq!(metadata.output_table, "source_view");
    assert_eq!(metadata.output_schema, "source.view.v1");
    assert_eq!(metadata.semantic.as_deref(), Some("source_view"));
    assert_eq!(metadata.materialize.as_deref(), Some("eager"));
    assert_eq!(metadata.backend, "sqlite-prototype");
    assert!(runner.materialized_metadata("missing_table").is_none());
}
