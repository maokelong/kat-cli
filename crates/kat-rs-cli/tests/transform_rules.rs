#[allow(dead_code)]
mod support;

use kat_rs_cli::trace_runtime::{
    adapter::sqlite::SQLiteDatasetAdapter,
    pack::spec::{InputTables, TransformOutputSpec, TransformSafetySpec, TransformSpec},
    transform::rules::run_rules_classify_transform,
};
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn rules_classify_transform_uses_pack_rule_set() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute("CREATE TABLE thread (itid INTEGER, thread_name TEXT)", [])
        .expect("thread table");
    conn.execute(
        "INSERT INTO thread VALUES (1, 'alpha-worker'), (2, 'beta-worker')",
        [],
    )
    .expect("thread rows");
    drop(conn);

    let pack = support::rules_pack(vec![(
        "alpha",
        json!({
            "field": "thread_name",
            "contains": "alpha"
        }),
    )]);
    let transform = test_transform(
        InputTables::List(vec!["thread".to_string()]),
        TransformSafetySpec {
            allowed_tables: vec!["thread".to_string()],
        },
    );
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    run_rules_classify_transform(&mut adapter, &pack, &transform).expect("rules transform runs");

    let rows = adapter
        .query_json("SELECT itid, class FROM thread_identity ORDER BY itid")
        .expect("query derived");
    assert_eq!(rows[0]["class"], "alpha");
    assert_eq!(rows[1]["class"], "unclassified");
}

#[test]
fn rules_classify_transform_rejects_empty_allowed_tables() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute("CREATE TABLE thread (itid INTEGER, thread_name TEXT)", [])
        .expect("thread table");
    drop(conn);

    let pack = support::rules_pack(vec![(
        "alpha",
        json!({
            "field": "thread_name",
            "contains": "alpha"
        }),
    )]);
    let transform = test_transform(
        InputTables::List(vec!["thread".to_string()]),
        Default::default(),
    );
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");

    let error = run_rules_classify_transform(&mut adapter, &pack, &transform)
        .expect_err("empty allowedTables is rejected");

    assert!(
        error.to_string().contains("safety.allowedTables"),
        "error: {error:#}"
    );
}

fn test_transform(inputs: InputTables, safety: TransformSafetySpec) -> TransformSpec {
    TransformSpec {
        id: "thread_identity".to_string(),
        kind: "rules.classify".to_string(),
        inputs,
        sql: None,
        params: Default::default(),
        bind: Default::default(),
        where_: Default::default(),
        output: TransformOutputSpec {
            table: "thread_identity".to_string(),
            schema: "thread.identity.v1".to_string(),
            semantic: None,
        },
        materialize: None,
        safety,
    }
}
