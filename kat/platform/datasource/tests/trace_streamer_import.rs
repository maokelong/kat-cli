use std::{fs, fs::File, path::Path};

use arrow_array::{Array, Float64Array, Int64Array, RecordBatch, StringArray};
use kat_datasource::{DatasetWriteTarget, import_trace_streamer, inspect_dataset};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use rusqlite::Connection;
use tempfile::tempdir;

#[test]
fn imports_tables_views_empty_relations_and_strict_types() {
    let temp = tempdir().unwrap();
    let database = temp.path().join("trace-streamer.db");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE facts (
                id INTEGER,
                ratio REAL,
                label TEXT,
                doubled INTEGER GENERATED ALWAYS AS (id * 2) STORED
            );
            INSERT INTO facts (id, ratio, label) VALUES (42, 3.5, 'hello'), (NULL, 7, NULL);
            CREATE TABLE empty_relation (id INTEGER);
            CREATE VIEW facts_view (id, label) AS SELECT id, label FROM facts;
            "#,
        )
        .unwrap();
    drop(connection);
    let dataset = temp.path().join("dataset");

    let imported =
        import_trace_streamer(&database, DatasetWriteTarget::new(&dataset, false)).unwrap();

    assert_eq!(imported.path(), dunce::canonicalize(&dataset).unwrap());
    let inspection = inspect_dataset(&dataset).unwrap();
    assert_eq!(
        inspection
            .tables()
            .iter()
            .map(|table| table.name())
            .collect::<Vec<_>>(),
        vec!["empty_relation", "facts", "facts_view"]
    );
    assert_eq!(
        inspection.tables()[1]
            .columns()
            .iter()
            .map(|column| (column.name(), column.data_type(), column.nullable()))
            .collect::<Vec<_>>(),
        vec![
            ("id", "Int64", true),
            ("ratio", "Float64", true),
            ("label", "Utf8", true),
            ("doubled", "Int64", true),
        ]
    );

    let batches = read_batches(&dataset.join("tables/facts.parquet"));
    assert_eq!(batches.iter().map(RecordBatch::num_rows).sum::<usize>(), 2);
    let batch = &batches[0];
    let ids = batch
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    let ratios = batch
        .column(1)
        .as_any()
        .downcast_ref::<Float64Array>()
        .unwrap();
    let labels = batch
        .column(2)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let doubled = batch
        .column(3)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    assert_eq!(ids.value(0), 42);
    assert!(ids.is_null(1));
    assert_eq!(ratios.value(0), 3.5);
    assert_eq!(ratios.value(1), 7.0);
    assert_eq!(labels.value(0), "hello");
    assert!(labels.is_null(1));
    assert_eq!(doubled.value(0), 84);
    assert!(doubled.is_null(1));
    assert_eq!(
        read_batches(&dataset.join("tables/empty_relation.parquet"))
            .iter()
            .map(RecordBatch::num_rows)
            .sum::<usize>(),
        0
    );
}

#[test]
fn overwrite_replaces_all_old_contents() {
    let temp = tempdir().unwrap();
    let database = temp.path().join("source.db");
    create_database(
        &database,
        "CREATE TABLE current (value INTEGER); INSERT INTO current VALUES (1);",
    );
    let dataset = temp.path().join("dataset");
    fs::create_dir(&dataset).unwrap();
    fs::write(dataset.join("unrecognized"), b"old").unwrap();

    let without_permission =
        import_trace_streamer(&database, DatasetWriteTarget::new(&dataset, false)).unwrap_err();
    assert!(error_chain(&without_permission).contains("not empty"));
    assert!(dataset.join("unrecognized").exists());

    import_trace_streamer(&database, DatasetWriteTarget::new(&dataset, true)).unwrap();
    assert!(!dataset.join("unrecognized").exists());
    assert!(dataset.join("tables/current.parquet").is_file());
}

#[test]
fn empty_database_and_invalid_relation_preflight_preserve_existing_target() {
    let temp = tempdir().unwrap();
    let dataset = temp.path().join("dataset");
    fs::create_dir(&dataset).unwrap();
    fs::write(dataset.join("sentinel"), "old Dataset").unwrap();

    for (name, schema) in [
        ("empty.db", ""),
        (
            "invalid-name.db",
            "CREATE TABLE \"bad-name\" (value INTEGER);",
        ),
        (
            "broken-view.db",
            "CREATE TABLE good (value INTEGER); CREATE VIEW broken AS SELECT * FROM missing;",
        ),
    ] {
        let database = temp.path().join(name);
        create_database(&database, schema);

        assert!(import_trace_streamer(&database, DatasetWriteTarget::new(&dataset, true)).is_err());
        assert_eq!(
            fs::read_to_string(dataset.join("sentinel")).unwrap(),
            "old Dataset"
        );
    }
}

#[test]
fn non_system_sqlite_prefix_is_materialized_exactly() {
    let temp = tempdir().unwrap();

    let prefixed_database = temp.path().join("prefixed.db");
    create_database(
        &prefixed_database,
        "CREATE TABLE sqlitex_user (value INTEGER);",
    );
    let prefixed_dataset = temp.path().join("prefixed-dataset");
    import_trace_streamer(
        &prefixed_database,
        DatasetWriteTarget::new(&prefixed_dataset, false),
    )
    .unwrap();
    assert_eq!(
        inspect_dataset(&prefixed_dataset).unwrap().tables()[0].name(),
        "sqlitex_user"
    );
}

#[test]
fn unsupported_schema_or_cell_aborts_without_marker() {
    for (schema, expected) in [
        (
            "CREATE TABLE bad (value BLOB);",
            "unsupported SQLite declared type",
        ),
        (
            "CREATE TABLE bad (value VARCHAR);",
            "unsupported SQLite declared type",
        ),
        (
            "CREATE TABLE bad (value REAL); INSERT INTO bad VALUES (9e999);",
            "cannot convert SQLite cell",
        ),
        (
            "CREATE TABLE bad (value TEXT); INSERT INTO bad VALUES (CAST(X'80' AS TEXT));",
            "cannot convert SQLite cell",
        ),
        (
            "CREATE TABLE a_good (value INTEGER); INSERT INTO a_good VALUES (1); \
             CREATE TABLE z_bad (value INTEGER); INSERT INTO z_bad VALUES ('not-an-integer');",
            "cannot convert SQLite cell",
        ),
    ] {
        let temp = tempdir().unwrap();
        let database = temp.path().join("source.db");
        create_database(&database, schema);
        let dataset = temp.path().join("dataset");

        let error =
            import_trace_streamer(&database, DatasetWriteTarget::new(&dataset, false)).unwrap_err();

        assert!(error.to_string().contains(expected), "unexpected: {error}");
        assert!(!dataset.join(".kat-dataset").exists());
        assert!(inspect_dataset(&dataset).is_err());
        if schema.contains("a_good") {
            assert!(dataset.join("tables/a_good.parquet").is_file());
        }
    }
}

#[test]
fn duplicate_columns_and_relation_failure_abort_without_marker() {
    for fixture in ["duplicate", "broken-view"] {
        let temp = tempdir().unwrap();
        let database = temp.path().join("source.db");
        let connection = Connection::open(&database).unwrap();
        if fixture == "duplicate" {
            connection
                .execute_batch(
                    "CREATE TABLE duplicate (first INTEGER, second TEXT); \
                     PRAGMA writable_schema=ON; \
                     UPDATE sqlite_schema SET sql='CREATE TABLE duplicate (value INTEGER, value TEXT)' WHERE name='duplicate'; \
                     PRAGMA writable_schema=OFF; \
                     PRAGMA schema_version=2;",
                )
                .unwrap();
        } else {
            connection
                .execute_batch(
                    "CREATE TABLE good (value INTEGER); \
                     CREATE VIEW broken AS SELECT * FROM missing;",
                )
                .unwrap();
        }
        drop(connection);
        let dataset = temp.path().join("dataset");

        assert!(
            import_trace_streamer(&database, DatasetWriteTarget::new(&dataset, false),).is_err()
        );
        assert!(!dataset.join(".kat-dataset").exists());
    }
}

fn create_database(path: &Path, schema: &str) {
    Connection::open(path)
        .unwrap()
        .execute_batch(schema)
        .unwrap();
}

fn read_batches(path: &Path) -> Vec<RecordBatch> {
    ParquetRecordBatchReaderBuilder::try_new(File::open(path).unwrap())
        .unwrap()
        .build()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn error_chain(error: &dyn std::error::Error) -> String {
    let mut messages = vec![error.to_string()];
    let mut source = error.source();
    while let Some(error) = source {
        messages.push(error.to_string());
        source = error.source();
    }
    messages.join("\n")
}
