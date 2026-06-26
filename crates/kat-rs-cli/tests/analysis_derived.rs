use kat_rs_cli::trace_runtime::{
    adapter::{DatasetAdapter, sqlite::SQLiteDatasetAdapter},
    analysis::derived::DerivedRunner,
    pack::load_pack,
};
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn derived_runner_materializes_requested_transforms_once() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw");
    conn.execute(
        "CREATE TABLE thread_state (itid INTEGER, ts INTEGER, dur INTEGER, state TEXT)",
        [],
    )
    .expect("thread_state");
    conn.execute("INSERT INTO thread_state VALUES (7, 10, 5, 'R')", [])
        .expect("row");
    drop(conn);

    let pack = load_pack(workspace_root().join("packs/openharmony-core")).expect("pack");
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let mut runner = DerivedRunner::new(&pack);

    runner
        .ensure_table(
            &mut adapter,
            "thread_state_segments",
            &json!({ "itid": 7 }),
            &json!({}),
        )
        .expect("first materialization");
    runner
        .ensure_table(
            &mut adapter,
            "thread_state_segments",
            &json!({ "itid": 7 }),
            &json!({}),
        )
        .expect("second materialization is no-op");

    assert!(
        adapter
            .table_exists("thread_state_segments")
            .expect("table exists")
    );
    let rows = adapter
        .query_json("SELECT itid, start_ts, end_ts, state_class FROM thread_state_segments")
        .expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["state_class"], "runnable");
}

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf()
}
