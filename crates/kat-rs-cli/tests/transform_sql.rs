#[allow(dead_code)]
mod support;

use std::fs;

use kat_rs_cli::trace_runtime::{
    adapter::sqlite::SQLiteDatasetAdapter, transform::sql::run_sql_view_transform,
};
use rusqlite::Connection;
use tempfile::tempdir;

#[test]
fn sql_view_transform_materializes_queryable_derived_table() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute(
        "CREATE TABLE thread_state (itid INTEGER, ts INTEGER, dur INTEGER, state TEXT)",
        [],
    )
    .expect("create raw table");
    conn.execute(
        "INSERT INTO thread_state VALUES (7, 10, 5, 'R'), (7, 15, 10, 'S'), (8, 20, 5, 'R')",
        [],
    )
    .expect("insert rows");
    drop(conn);

    fs::write(
        dir.path().join("segments.sql"),
        "SELECT itid, ts AS start_ts, ts + dur AS end_ts, state FROM thread_state WHERE itid = ${itid}",
    )
    .expect("sql file");

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let mut spec = support::sql_transform(
        "segments",
        "segments.sql",
        "derived_output",
        vec!["thread_state"],
        vec!["thread_state"],
    );
    spec.materialize = Some("eager".to_string());

    run_sql_view_transform(
        &mut adapter,
        dir.path(),
        &spec,
        &serde_json::json!({ "itid": 7 }),
    )
    .expect("transform runs");

    let rows = adapter
        .query_json("SELECT * FROM derived_output ORDER BY start_ts")
        .expect("query derived");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["start_ts"], 10);
    assert_eq!(rows[1]["state"], "S");
}

#[test]
fn sql_view_transform_rejects_disallowed_table_reference() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute("CREATE TABLE thread_state (itid INTEGER)", [])
        .expect("thread_state");
    conn.execute("CREATE TABLE secret_table (value INTEGER)", [])
        .expect("secret_table");
    drop(conn);
    fs::write(
        dir.path().join("bad.sql"),
        "SELECT * FROM thread_state UNION ALL SELECT * FROM secret_table",
    )
    .expect("sql file");

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let spec = support::sql_transform(
        "bad",
        "bad.sql",
        "bad_output",
        vec!["thread_state"],
        vec!["thread_state"],
    );

    let error = run_sql_view_transform(&mut adapter, dir.path(), &spec, &serde_json::json!({}))
        .expect_err("disallowed table is rejected");
    assert!(
        error.to_string().contains("outside safety.allowedTables"),
        "error: {error:#}"
    );
}

#[test]
fn sql_view_transform_rejects_parent_directory_sql_path() {
    let dir = tempdir().expect("tempdir");
    let outside_sql = dir
        .path()
        .parent()
        .expect("tempdir parent")
        .join("outside.sql");
    fs::write(&outside_sql, "SELECT itid FROM thread_state").expect("outside sql file");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute("CREATE TABLE thread_state (itid INTEGER)", [])
        .expect("thread_state");
    drop(conn);

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let spec = support::sql_transform(
        "unsafe_path",
        "../outside.sql",
        "unsafe_path",
        vec!["thread_state"],
        vec!["thread_state"],
    );

    let error = run_sql_view_transform(&mut adapter, dir.path(), &spec, &serde_json::json!({}))
        .expect_err("parent directory sql path is rejected");
    assert!(
        error.to_string().contains("unsafe sql path"),
        "error: {error:#}"
    );
}

#[test]
fn sql_view_transform_rejects_duplicate_output_table() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute("CREATE TABLE thread_state (itid INTEGER)", [])
        .expect("thread_state");
    conn.execute("INSERT INTO thread_state VALUES (7)", [])
        .expect("insert row");
    drop(conn);
    fs::write(
        dir.path().join("segments.sql"),
        "SELECT itid FROM thread_state",
    )
    .expect("sql file");

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let spec = support::sql_transform(
        "segments",
        "segments.sql",
        "segments",
        vec!["thread_state"],
        vec!["thread_state"],
    );

    run_sql_view_transform(&mut adapter, dir.path(), &spec, &serde_json::json!({}))
        .expect("first transform runs");
    let error = run_sql_view_transform(&mut adapter, dir.path(), &spec, &serde_json::json!({}))
        .expect_err("duplicate output table is rejected");
    assert!(
        error.to_string().contains("output table already exists"),
        "error: {error:#}"
    );
}

#[test]
fn sql_view_transform_allows_cte_aliases_not_in_allowed_tables() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute("CREATE TABLE thread_state (itid INTEGER, ts INTEGER)", [])
        .expect("thread_state");
    conn.execute("INSERT INTO thread_state VALUES (7, 10), (8, 20)", [])
        .expect("insert rows");
    drop(conn);
    fs::write(
        dir.path().join("recent.sql"),
        "WITH recent AS (SELECT * FROM thread_state) SELECT * FROM recent WHERE itid = 7",
    )
    .expect("sql file");

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let spec = support::sql_transform(
        "recent",
        "recent.sql",
        "recent_output",
        vec!["thread_state"],
        vec!["thread_state"],
    );

    run_sql_view_transform(&mut adapter, dir.path(), &spec, &serde_json::json!({}))
        .expect("cte alias is allowed");
    let rows = adapter
        .query_json("SELECT * FROM recent_output")
        .expect("query output");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["itid"], 7);
}

#[test]
fn sql_view_transform_rejects_qualified_table_matching_cte_alias() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute("CREATE TABLE thread_state (itid INTEGER)", [])
        .expect("thread_state");
    conn.execute("CREATE TABLE secret_table (itid INTEGER)", [])
        .expect("secret_table");
    drop(conn);
    fs::write(
        dir.path().join("qualified_secret.sql"),
        "WITH secret_table AS (SELECT * FROM thread_state) SELECT * FROM raw.secret_table",
    )
    .expect("sql file");

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let spec = support::sql_transform(
        "qualified_secret",
        "qualified_secret.sql",
        "qualified_secret_output",
        vec!["thread_state"],
        vec!["thread_state"],
    );

    let error = run_sql_view_transform(&mut adapter, dir.path(), &spec, &serde_json::json!({}))
        .expect_err("qualified raw table reference is rejected");
    assert!(
        error.to_string().contains("outside safety.allowedTables"),
        "error: {error:#}"
    );
}
