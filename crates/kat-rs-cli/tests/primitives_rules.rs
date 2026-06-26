use kat_rs_cli::trace_runtime::{
    adapter::{DatasetAdapter, sqlite::SQLiteDatasetAdapter},
    primitives::rules::{ClassifyRuleSet, run_rules_classify},
};
use rusqlite::Connection;
use tempfile::tempdir;

#[test]
fn rules_classify_uses_pack_rules_to_create_identity_table() {
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

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let rules = ClassifyRuleSet {
        source_table: "thread".to_string(),
        output_table: "thread_identity".to_string(),
        id_column: "itid".to_string(),
        text_column: "thread_name".to_string(),
        rules: vec![
            ("alpha".to_string(), vec!["alpha".to_string()], Vec::new()),
            ("beta".to_string(), vec!["beta".to_string()], Vec::new()),
        ],
    };

    run_rules_classify(&mut adapter, &rules).expect("classify runs");

    let rows = DatasetAdapter::query_json(
        &mut adapter,
        "SELECT itid, class FROM thread_identity ORDER BY itid",
    )
    .expect("query identity");
    assert_eq!(rows[0]["class"], "alpha");
    assert_eq!(rows[1]["class"], "beta");
}

#[test]
fn rules_classify_empty_rules_marks_all_rows_unclassified() {
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

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let rules = ClassifyRuleSet {
        source_table: "thread".to_string(),
        output_table: "thread_identity".to_string(),
        id_column: "itid".to_string(),
        text_column: "thread_name".to_string(),
        rules: Vec::new(),
    };

    run_rules_classify(&mut adapter, &rules).expect("classify runs");

    let rows = DatasetAdapter::query_json(
        &mut adapter,
        "SELECT itid, class FROM thread_identity ORDER BY itid",
    )
    .expect("query identity");
    assert_eq!(rows[0]["class"], "unclassified");
    assert_eq!(rows[1]["class"], "unclassified");
}
