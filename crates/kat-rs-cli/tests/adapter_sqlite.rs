use kat_rs_cli::trace_runtime::adapter::{DatasetAdapter, sqlite::SQLiteDatasetAdapter};
use rusqlite::Connection;
use tempfile::tempdir;

#[test]
fn sqlite_adapter_filters_internal_raw_sqlite_tables() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    conn.execute(
        "CREATE TABLE events (id INTEGER PRIMARY KEY AUTOINCREMENT, value TEXT)",
        [],
    )
    .expect("create autoincrement table");
    conn.execute("INSERT INTO events (value) VALUES ('first')", [])
        .expect("insert row");
    drop(conn);

    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter opens");
    let tables = adapter.table_names().expect("table names");

    assert!(tables.iter().any(|name| name == "events"), "{tables:?}");
    assert!(
        !tables.iter().any(|name| name == "sqlite_sequence"),
        "{tables:?}"
    );
}
