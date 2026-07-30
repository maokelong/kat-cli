use std::{fs, fs::File, path::Path};

use arrow_array::{Array, Float64Array, Int64Array, RecordBatch, StringArray};
use kat_datasource::{
    DatasetWriteTarget, TraceStreamerImportError, import_deprecated_trace_streamer, inspect_dataset,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use rusqlite::Connection;
use tempfile::tempdir;

#[test]
fn lossy_real_cell_error_reports_the_exact_integer() {
    assert_eq!(
        TraceStreamerImportError::LossyRealCell {
            relation: "facts".to_owned(),
            column: "ratio".to_owned(),
            row: 2,
            value: (1_i64 << 53) + 1,
        }
        .to_string(),
        "cannot convert SQLite cell facts.ratio at row 2: INTEGER 9007199254740993 cannot be represented exactly as Float64"
    );
}

#[test]
fn imports_tables_skips_views_empty_relations_and_strict_types() {
    let temp = tempdir().unwrap();
    let database = temp.path().join("trace-streamer.db");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE facts (
                id INT,
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
        import_deprecated_trace_streamer(&database, DatasetWriteTarget::write_to_empty(&dataset))
            .unwrap();

    assert_eq!(imported.path(), dunce::canonicalize(&dataset).unwrap());
    let inspection = inspect_dataset(&dataset).unwrap();
    assert_eq!(
        inspection
            .tables()
            .iter()
            .map(|table| table.name())
            .collect::<Vec<_>>(),
        vec!["empty_relation", "facts"]
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
        import_deprecated_trace_streamer(&database, DatasetWriteTarget::write_to_empty(&dataset))
            .unwrap_err();
    assert!(error_chain(&without_permission).contains("not empty"));
    assert!(dataset.join("unrecognized").exists());

    import_deprecated_trace_streamer(
        &database,
        DatasetWriteTarget::permanently_replace_all_contents(&dataset),
    )
    .unwrap();
    assert!(!dataset.join("unrecognized").exists());
    assert!(dataset.join("tables/current.parquet").is_file());
}

#[cfg(windows)]
#[test]
fn partial_overwrite_failure_invalidates_the_dataset_marker() {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};

    let temp = tempdir().unwrap();
    let database = temp.path().join("source.db");
    create_database(
        &database,
        "CREATE TABLE current (value INTEGER); INSERT INTO current VALUES (1);",
    );
    let dataset = temp.path().join("dataset");
    import_deprecated_trace_streamer(&database, DatasetWriteTarget::write_to_empty(&dataset))
        .unwrap();
    let blocked = dataset.join("blocked-entry");
    fs::write(&blocked, "cannot delete while open").unwrap();
    let _locked = OpenOptions::new()
        .read(true)
        .share_mode(0x0000_0001 | 0x0000_0002)
        .open(blocked)
        .unwrap();

    let failure = import_deprecated_trace_streamer(
        &database,
        DatasetWriteTarget::permanently_replace_all_contents(&dataset),
    )
    .unwrap_err();

    assert!(matches!(failure, TraceStreamerImportError::WriteDataset(_)));
    assert!(!dataset.join(".kat-dataset").exists());
    assert!(inspect_dataset(&dataset).is_err());
}

#[test]
fn empty_database_and_invalid_relation_name_preserve_existing_target() {
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
    ] {
        let database = temp.path().join(name);
        create_database(&database, schema);

        assert!(
            import_deprecated_trace_streamer(
                &database,
                DatasetWriteTarget::permanently_replace_all_contents(&dataset),
            )
            .is_err()
        );
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
    import_deprecated_trace_streamer(
        &prefixed_database,
        DatasetWriteTarget::write_to_empty(&prefixed_dataset),
    )
    .unwrap();
    assert_eq!(
        inspect_dataset(&prefixed_dataset).unwrap().tables()[0].name(),
        "sqlitex_user"
    );
}

#[test]
fn unsupported_schema_or_cell_aborts_without_marker() {
    enum ExpectedFailure {
        UnsupportedDeclaredType,
        NonFiniteReal,
        InvalidUtf8Text,
        StorageClassMismatch,
    }

    for (schema, expected) in [
        (
            "CREATE TABLE bad (value BLOB);",
            ExpectedFailure::UnsupportedDeclaredType,
        ),
        (
            "CREATE TABLE bad (value VARCHAR);",
            ExpectedFailure::UnsupportedDeclaredType,
        ),
        (
            "CREATE TABLE bad (value REAL); INSERT INTO bad VALUES (9e999);",
            ExpectedFailure::NonFiniteReal,
        ),
        (
            "CREATE TABLE bad (value TEXT); INSERT INTO bad VALUES (CAST(X'80' AS TEXT));",
            ExpectedFailure::InvalidUtf8Text,
        ),
        (
            "CREATE TABLE a_good (value INTEGER); INSERT INTO a_good VALUES (1); \
             CREATE TABLE z_bad (value INTEGER); INSERT INTO z_bad VALUES ('not-an-integer');",
            ExpectedFailure::StorageClassMismatch,
        ),
    ] {
        let temp = tempdir().unwrap();
        let database = temp.path().join("source.db");
        create_database(&database, schema);
        let dataset = temp.path().join("dataset");

        let error = import_deprecated_trace_streamer(
            &database,
            DatasetWriteTarget::write_to_empty(&dataset),
        )
        .unwrap_err();

        match expected {
            ExpectedFailure::UnsupportedDeclaredType => assert!(matches!(
                error,
                TraceStreamerImportError::UnsupportedDeclaredType { .. }
            )),
            ExpectedFailure::NonFiniteReal => assert!(matches!(
                error,
                TraceStreamerImportError::NonFiniteRealCell {
                    relation,
                    column,
                    row: 1,
                    value,
                } if relation == "bad" && column == "value" && value.is_infinite()
            )),
            ExpectedFailure::InvalidUtf8Text => assert!(matches!(
                error,
                TraceStreamerImportError::InvalidUtf8TextCell {
                    relation,
                    column,
                    row: 1,
                    source,
                } if relation == "bad"
                    && column == "value"
                    && source.valid_up_to() == 0
                    && source.error_len() == Some(1)
            )),
            ExpectedFailure::StorageClassMismatch => assert!(matches!(
                error,
                TraceStreamerImportError::ConvertCell {
                    relation,
                    column,
                    row: 1,
                    storage_class: "TEXT",
                } if relation == "z_bad" && column == "value"
            )),
        }
        assert!(!dataset.join(".kat-dataset").exists());
        assert!(inspect_dataset(&dataset).is_err());
        if schema.contains("a_good") {
            assert!(dataset.join("tables/a_good.parquet").is_file());
        }
    }
}

#[test]
fn duplicate_columns_abort_without_marker() {
    let temp = tempdir().unwrap();
    let database = temp.path().join("source.db");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE duplicate (first INTEGER, second TEXT); \
             PRAGMA writable_schema=ON; \
             UPDATE sqlite_schema SET sql='CREATE TABLE duplicate (value INTEGER, value TEXT)' WHERE name='duplicate'; \
             PRAGMA writable_schema=OFF; \
             PRAGMA schema_version=2;",
        )
        .unwrap();
    drop(connection);
    let dataset = temp.path().join("dataset");

    assert!(
        import_deprecated_trace_streamer(
            &database,
            DatasetWriteTarget::write_to_empty(&dataset),
        )
        .is_err()
    );
    assert!(!dataset.join(".kat-dataset").exists());
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
