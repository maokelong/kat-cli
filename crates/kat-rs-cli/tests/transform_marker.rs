#[allow(dead_code)]
mod support;

use std::collections::BTreeMap;

use kat_rs_cli::trace_runtime::{
    adapter::sqlite::SQLiteDatasetAdapter,
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
    conn.execute(
        "INSERT INTO process VALUES (89, 15040, '.tencent.wechat')",
        [],
    )
    .expect("process row");
    conn.execute(
        "INSERT INTO thread VALUES (405, 15040, 89, '.tencent.wechat', 1)",
        [],
    )
    .expect("thread row");
    conn.execute(
        "INSERT INTO callstack VALUES
            (30493, 405, NULL, 'H:UIVsyncTask[timestamp:246302563097][vsyncID:3269]|M0539'),
            (30754, 405, 30493, 'H:UIVsyncTask[timestamp:246302563097][vsyncID:3269][layoutMeasureDurationStartTimestamp:246307034375][layoutMeasureDurationEndTimestamp:246329389063][firstDrawFrame:1]|M0539')",
        [],
    )
    .expect("callstack rows");
    drop(conn);

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

    let transform = TransformSpec {
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
    };

    let params = json!({
        "marker": "firstDrawFrame:1",
        "target_process": ".tencent.wechat"
    });
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    run_marker_extract_bracket_fields_transform(&mut adapter, &transform, &params, &json!({}))
        .expect("marker transform");

    let rows = adapter
        .query_json("SELECT callstack_id, root_callstack_id, itid, process_name, vsync_id, start_ts, end_ts, dur_ns FROM first_draw_window")
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
}
