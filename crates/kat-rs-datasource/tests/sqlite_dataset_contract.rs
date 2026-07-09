use std::{fs, path::Path};

use kat_rs_datasource::{TraceDatasource, inspect_dataset_tables, materialize_sqlite_dataset};
use rusqlite::{Connection, params};
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn sqlite_dataset_rejects_missing_source_database_without_creating_it() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("missing.db");
    let dataset_path = dir.path().join("dataset");

    assert!(!db_path.exists(), "source db should start missing");

    let result = materialize_sqlite_dataset(&db_path, &dataset_path).await;
    assert!(
        result.is_err(),
        "missing source db should fail materialization"
    );
    assert!(
        !db_path.exists(),
        "materialization must not create the missing source db"
    );
}

#[tokio::test]
async fn sqlite_dataset_materializes_tables_and_queries_after_source_is_removed() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("trace.db");
    create_trace_streamer_like_db(&db_path);
    let dataset_path = dir.path().join("dataset");

    materialize_sqlite_dataset(&db_path, &dataset_path)
        .await
        .expect("sqlite dataset materializes");
    fs::remove_file(&db_path).expect("source db can be removed");

    let tables = inspect_dataset_tables(&dataset_path).expect("dataset inspects");
    assert!(tables.iter().any(|table| table.name == "thread_state"));
    assert!(tables.iter().any(|table| table.name == "empty_table"));

    let datasource = TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let rows = datasource
        .query_json(
            "select itid, tid, pid, state, ts, dur \
             from thread_state order by id",
        )
        .await
        .expect("query works");

    assert_eq!(
        rows,
        json!([
            {
                "itid": 405,
                "tid": 15040,
                "pid": 15040,
                "state": "S",
                "ts": 1000,
                "dur": 500
            },
            {
                "itid": 405,
                "tid": 15040,
                "pid": 15040,
                "state": "Running",
                "ts": 1500,
                "dur": 300
            }
        ])
    );

    let empty_rows = datasource
        .query_json("select count(*) as count from empty_table")
        .await
        .expect("empty table query works");
    assert_eq!(empty_rows, json!([{ "count": 0 }]));
}

#[tokio::test]
async fn sqlite_dataset_preserves_null_real_text_and_blob_values() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("types.db");
    let connection = Connection::open(&db_path).expect("sqlite opens");
    connection
        .execute(
            "create table sample_types(
                id integer,
                score real,
                label text,
                payload blob,
                missing text
            )",
            [],
        )
        .expect("table created");
    connection
        .execute(
            "insert into sample_types values (?1, ?2, ?3, ?4, null)",
            params![7_i64, 1.5_f64, "hello", vec![0x01_u8, 0x02_u8, 0xff_u8]],
        )
        .expect("row inserted");
    drop(connection);

    let dataset_path = dir.path().join("dataset");
    materialize_sqlite_dataset(&db_path, &dataset_path)
        .await
        .expect("sqlite dataset materializes");

    let datasource = TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let rows = datasource
        .query_json("select id, score, label, payload, missing from sample_types")
        .await
        .expect("query works");

    assert_eq!(
        rows,
        json!([{
            "id": 7,
            "score": 1.5,
            "label": "hello",
            "payload": "0102ff",
            "missing": null
        }])
    );
}

#[tokio::test]
async fn sqlite_dataset_rejects_real_values_in_integer_columns() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("types.db");
    let connection = Connection::open(&db_path).expect("sqlite opens");
    connection
        .execute("create table sample_types(id integer)", [])
        .expect("table created");
    connection
        .execute("insert into sample_types values (?1)", params![1.5_f64])
        .expect("row inserted");
    drop(connection);

    let dataset_path = dir.path().join("dataset");
    let error = materialize_sqlite_dataset(&db_path, &dataset_path)
        .await
        .expect_err("real value in integer column is rejected");

    assert!(
        format!("{error:#}").contains("sample_types.id"),
        "{error:#}"
    );
}

fn create_trace_streamer_like_db(path: &Path) {
    let connection = Connection::open(path).expect("sqlite opens");
    connection
        .execute(
            "create table thread_state(
                id integer,
                ts integer,
                dur integer,
                cpu integer,
                itid integer,
                tid integer,
                pid integer,
                state text,
                arg_setid integer
            )",
            [],
        )
        .expect("thread_state table created");
    connection
        .execute(
            "insert into thread_state values
             (1, 1000, 500, null, 405, 15040, 15040, 'S', null),
             (2, 1500, 300, 4, 405, 15040, 15040, 'Running', null)",
            [],
        )
        .expect("thread_state rows inserted");
    connection
        .execute("create table empty_table(id integer, name text)", [])
        .expect("empty table created");
}
