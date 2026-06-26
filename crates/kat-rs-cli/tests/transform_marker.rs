#[allow(dead_code)]
mod support;

use std::collections::BTreeMap;

use kat_rs_cli::trace_runtime::{
    adapter::{DatasetAdapter, sqlite::SQLiteDatasetAdapter},
    pack::spec::{
        InputTables, MarkerSourceSpec, TransformOutputSpec, TransformSafetySpec, TransformSpec,
    },
    transform::marker::run_marker_extract_bracket_fields_transform,
};
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn marker_transform_extracts_first_draw_window_from_callstack_name() {
    let (_dir, raw_db, scratch_db) = marker_fixture();
    let transform = marker_transform();
    let params = default_params();
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    run_marker_extract_bracket_fields_transform(&mut adapter, &transform, &params, &json!({}))
        .expect("marker transform");

    let rows = adapter
        .query_json("SELECT callstack_id, root_callstack_id, itid, process_name, vsync_id, start_ts, end_ts, dur_ns, marker_name FROM first_draw_window")
        .expect("query first draw");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["callstack_id"], 30754);
    assert_eq!(rows[0]["root_callstack_id"], 30493);
    assert_eq!(rows[0]["itid"], 405);
    assert_eq!(rows[0]["process_name"], ".tencent.wechat");
    assert_eq!(rows[0]["vsync_id"], 3269);
    assert_eq!(rows[0]["start_ts"], 246307034375i64);
    assert_eq!(rows[0]["end_ts"], 246329389063i64);
    assert_eq!(rows[0]["dur_ns"], 22354688i64);
    assert_eq!(
        rows[0]["marker_name"],
        "H:UIVsyncTask[timestamp:246302563097][vsyncID:3269][layoutMeasureDurationStartTimestamp:246307034375][layoutMeasureDurationEndTimestamp:246329389063][firstDrawFrame:1]|M0539"
    );
}

#[test]
fn marker_transform_rejects_missing_declared_inputs_without_output() {
    let (_dir, raw_db, scratch_db) = marker_fixture();
    let mut transform = marker_transform();
    transform.inputs = InputTables::List(vec!["callstack".to_string(), "thread".to_string()]);
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_marker_extract_bracket_fields_transform(
        &mut adapter,
        &transform,
        &default_params(),
        &json!({}),
    )
    .expect_err("missing declared input is rejected");

    assert!(error.to_string().contains("inputs"), "error: {error:#}");
    assert_no_output_table(&mut adapter, &transform);
}

#[test]
fn marker_transform_rejects_empty_declared_inputs_without_output() {
    let (_dir, raw_db, scratch_db) = marker_fixture();
    let mut transform = marker_transform();
    transform.inputs = InputTables::Empty;
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_marker_extract_bracket_fields_transform(
        &mut adapter,
        &transform,
        &default_params(),
        &json!({}),
    )
    .expect_err("empty declared inputs are rejected");

    assert!(error.to_string().contains("inputs"), "error: {error:#}");
    assert_no_output_table(&mut adapter, &transform);
}

#[test]
fn marker_transform_rejects_unsupported_joins_without_output() {
    let (_dir, raw_db, scratch_db) = marker_fixture();
    let mut transform = marker_transform();
    transform.joins.insert(
        "thread".to_string(),
        [("itid".to_string(), "callid".to_string())].into(),
    );
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_marker_extract_bracket_fields_transform(
        &mut adapter,
        &transform,
        &default_params(),
        &json!({}),
    )
    .expect_err("unsupported joins are rejected");

    assert!(error.to_string().contains("joins"), "error: {error:#}");
    assert_no_output_table(&mut adapter, &transform);
}

#[test]
fn marker_transform_rejects_unsupported_filter_keys_without_output() {
    let (_dir, raw_db, scratch_db) = marker_fixture();
    let mut transform = marker_transform();
    transform
        .filters
        .insert("thread_name".to_string(), json!(".tencent.wechat"));
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_marker_extract_bracket_fields_transform(
        &mut adapter,
        &transform,
        &default_params(),
        &json!({}),
    )
    .expect_err("unsupported filter key is rejected");

    assert!(error.to_string().contains("filter"), "error: {error:#}");
    assert_no_output_table(&mut adapter, &transform);
}

#[test]
fn marker_transform_rejects_missing_required_fields_without_output() {
    let (_dir, raw_db, scratch_db) = marker_fixture();
    let mut transform = marker_transform();
    transform.fields.remove("start_ts");
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_marker_extract_bracket_fields_transform(
        &mut adapter,
        &transform,
        &default_params(),
        &json!({}),
    )
    .expect_err("missing required field is rejected");

    assert!(error.to_string().contains("start_ts"), "error: {error:#}");
    assert_no_output_table(&mut adapter, &transform);
}

#[test]
fn marker_transform_rejects_empty_marker_without_output() {
    let (_dir, raw_db, scratch_db) = marker_fixture();
    let transform = marker_transform();
    let params = json!({
        "marker": "  ",
        "target_process": ".tencent.wechat"
    });
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error =
        run_marker_extract_bracket_fields_transform(&mut adapter, &transform, &params, &json!({}))
            .expect_err("empty marker is rejected");

    assert!(
        error.to_string().contains("source.contains"),
        "error: {error:#}"
    );
    assert_no_output_table(&mut adapter, &transform);
}

#[test]
fn marker_transform_rejects_empty_required_field_key_without_output() {
    let (_dir, raw_db, scratch_db) = marker_fixture();
    let mut transform = marker_transform();
    transform
        .fields
        .insert("start_ts".to_string(), " ".to_string());
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_marker_extract_bracket_fields_transform(
        &mut adapter,
        &transform,
        &default_params(),
        &json!({}),
    )
    .expect_err("empty field key is rejected");

    assert!(error.to_string().contains("start_ts"), "error: {error:#}");
    assert_no_output_table(&mut adapter, &transform);
}

#[test]
fn marker_transform_rejects_missing_safety_tables_without_output() {
    let (_dir, raw_db, scratch_db) = marker_fixture();
    let mut transform = marker_transform();
    transform.safety.allowed_tables = vec!["callstack".to_string(), "thread".to_string()];
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_marker_extract_bracket_fields_transform(
        &mut adapter,
        &transform,
        &default_params(),
        &json!({}),
    )
    .expect_err("missing safety table is rejected");

    assert!(
        error.to_string().contains("safety.allowedTables"),
        "error: {error:#}"
    );
    assert_no_output_table(&mut adapter, &transform);
}

#[test]
fn marker_transform_rejects_unsupported_source_without_output() {
    let (_dir, raw_db, scratch_db) = marker_fixture();
    let mut transform = marker_transform();
    transform.source = Some(MarkerSourceSpec {
        table: "thread".to_string(),
        column: "thread_name".to_string(),
        contains: "${params.marker}".to_string(),
    });
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_marker_extract_bracket_fields_transform(
        &mut adapter,
        &transform,
        &default_params(),
        &json!({}),
    )
    .expect_err("unsupported source is rejected");

    assert!(
        error.to_string().contains("callstack.name"),
        "error: {error:#}"
    );
    assert_no_output_table(&mut adapter, &transform);
}

#[test]
fn marker_transform_escapes_marker_and_process_name_literals() {
    let (_dir, raw_db, scratch_db) = marker_fixture_with_values(
        "we'chat",
        "H:UIVsyncTask[timestamp:246302563097][vsyncID:3269][layoutMeasureDurationStartTimestamp:246307034375][layoutMeasureDurationEndTimestamp:246329389063][firstDrawFrame:1][label:Bob's marker]|M0539",
    );
    let transform = marker_transform();
    let params = json!({
        "marker": "Bob's marker",
        "target_process": "we'chat"
    });
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    run_marker_extract_bracket_fields_transform(&mut adapter, &transform, &params, &json!({}))
        .expect("marker transform");

    let rows = adapter
        .query_json("SELECT process_name, marker_name FROM first_draw_window")
        .expect("query first draw");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["process_name"], "we'chat");
    assert_eq!(
        rows[0]["marker_name"],
        "H:UIVsyncTask[timestamp:246302563097][vsyncID:3269][layoutMeasureDurationStartTimestamp:246307034375][layoutMeasureDurationEndTimestamp:246329389063][firstDrawFrame:1][label:Bob's marker]|M0539"
    );
}

fn marker_fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    marker_fixture_with_values(
        ".tencent.wechat",
        "H:UIVsyncTask[timestamp:246302563097][vsyncID:3269][layoutMeasureDurationStartTimestamp:246307034375][layoutMeasureDurationEndTimestamp:246329389063][firstDrawFrame:1]|M0539",
    )
}

fn marker_fixture_with_values(
    process_name: &str,
    marker_name: &str,
) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute(
        "CREATE TABLE process (ipid INTEGER, pid INTEGER, name TEXT)",
        [],
    )
    .expect("process");
    conn.execute(
        "CREATE TABLE thread (itid INTEGER, tid INTEGER, ipid INTEGER, thread_name TEXT, is_main_thread INTEGER)",
        [],
    )
    .expect("thread");
    conn.execute(
        "CREATE TABLE callstack (id INTEGER, callid INTEGER, parent_id INTEGER, name TEXT)",
        [],
    )
    .expect("callstack");
    conn.execute("INSERT INTO process VALUES (89, 15040, ?1)", [process_name])
        .expect("process row");
    conn.execute(
        "INSERT INTO thread VALUES (405, 15040, 89, ?1, 1)",
        [process_name],
    )
    .expect("thread row");
    conn.execute(
        "INSERT INTO callstack VALUES
            (30493, 405, NULL, 'H:UIVsyncTask[timestamp:246302563097][vsyncID:3269]|M0539'),
            (30754, 405, 30493, ?1)",
        [marker_name],
    )
    .expect("callstack rows");
    drop(conn);
    (dir, raw_db, scratch_db)
}

fn marker_transform() -> TransformSpec {
    let mut fields = BTreeMap::new();
    fields.insert(
        "start_ts".to_string(),
        "layoutMeasureDurationStartTimestamp".to_string(),
    );
    fields.insert(
        "end_ts".to_string(),
        "layoutMeasureDurationEndTimestamp".to_string(),
    );
    fields.insert("vsync_id".to_string(), "vsyncID".to_string());

    let mut filters = BTreeMap::new();
    filters.insert(
        "process_name".to_string(),
        json!("${params.target_process}"),
    );

    TransformSpec {
        id: "first_draw_window".to_string(),
        kind: "marker.extract_bracket_fields".to_string(),
        inputs: InputTables::List(vec![
            "callstack".to_string(),
            "thread".to_string(),
            "process".to_string(),
        ]),
        sql: None,
        params: Default::default(),
        bind: Default::default(),
        where_: Default::default(),
        source: Some(MarkerSourceSpec {
            table: "callstack".to_string(),
            column: "name".to_string(),
            contains: "${params.marker}".to_string(),
        }),
        fields,
        joins: Default::default(),
        filters,
        output: TransformOutputSpec {
            table: "first_draw_window".to_string(),
            schema: "marker.first_draw_window.v1".to_string(),
            semantic: None,
        },
        materialize: None,
        safety: TransformSafetySpec {
            allowed_tables: vec![
                "callstack".to_string(),
                "thread".to_string(),
                "process".to_string(),
            ],
        },
    }
}

fn default_params() -> serde_json::Value {
    json!({
        "marker": "firstDrawFrame:1",
        "target_process": ".tencent.wechat"
    })
}

fn assert_no_output_table(adapter: &mut SQLiteDatasetAdapter, transform: &TransformSpec) {
    assert!(
        !adapter
            .table_exists(&transform.output.table)
            .expect("output table check")
    );
}
