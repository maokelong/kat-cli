use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;

use kat_datasource::{
    DatasetInspection, DatasetInspectionError, ResolvedTable, TableInspection, inspect_dataset,
    resolve_dataset,
};

fn dataset(root: &Path) -> PathBuf {
    let path = root.join("dataset");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join(".kat-dataset"), []).unwrap();
    path
}

fn parquet(path: &Path, fields: Vec<Field>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = File::create(path).unwrap();
    ArrowWriter::try_new(file, Arc::new(Schema::new(fields)), None)
        .unwrap()
        .close()
        .unwrap();
}

fn error(result: Result<DatasetInspection, DatasetInspectionError>) -> DatasetInspectionError {
    match result {
        Ok(_) => panic!("expected Dataset inspection failure"),
        Err(error) => error,
    }
}

#[test]
fn inspection_returns_canonical_path_sorted_tables_and_ordered_columns() {
    let temporary = tempfile::tempdir().unwrap();
    let dataset = dataset(temporary.path());
    parquet(
        &dataset.join("tables/zeta.parquet"),
        vec![Field::new("value", DataType::UInt64, false)],
    );
    parquet(
        &dataset.join("tables/alpha.parquet"),
        vec![
            Field::new("id", DataType::Int64, false),
            Field::new("label", DataType::Utf8, true),
        ],
    );
    fs::write(dataset.join("tables/bad-name.parquet"), "ignored").unwrap();
    fs::write(dataset.join("tables/readme.txt"), "ignored").unwrap();
    fs::create_dir(dataset.join("tables/nested.parquet")).unwrap();

    let inspection = inspect_dataset(&dataset).expect("inspect Dataset");

    assert_eq!(inspection.path(), dunce::canonicalize(&dataset).unwrap());
    assert_eq!(
        inspection
            .tables()
            .iter()
            .map(TableInspection::name)
            .collect::<Vec<_>>(),
        ["alpha", "zeta"]
    );
    let columns = inspection.tables()[0].columns();
    assert_eq!(columns[0].name(), "id");
    assert_eq!(columns[0].data_type(), "Int64");
    assert!(!columns[0].nullable());
    assert_eq!(columns[1].name(), "label");
    assert_eq!(columns[1].data_type(), "Utf8");
    assert!(columns[1].nullable());

    let resolved = resolve_dataset(&dataset).unwrap();
    assert_eq!(resolved.path(), inspection.path());
    assert_eq!(
        resolved
            .tables()
            .iter()
            .map(ResolvedTable::name)
            .collect::<Vec<_>>(),
        ["alpha", "zeta"]
    );
    assert_eq!(
        resolved.tables()[0].path(),
        dunce::canonicalize(dataset.join("tables/alpha.parquet")).unwrap()
    );
}

#[test]
fn missing_tables_directory_is_a_valid_empty_dataset() {
    let temporary = tempfile::tempdir().unwrap();
    let dataset = dataset(temporary.path());

    let inspection = inspect_dataset(&dataset).unwrap();

    assert!(inspection.tables().is_empty());
    assert!(!dataset.join("tables").exists());
}

#[test]
fn marker_must_be_an_empty_regular_file() {
    for marker in ["missing", "non-empty", "directory"] {
        let temporary = tempfile::tempdir().unwrap();
        let dataset = temporary.path().join("dataset");
        fs::create_dir(&dataset).unwrap();
        match marker {
            "non-empty" => fs::write(dataset.join(".kat-dataset"), "content").unwrap(),
            "directory" => fs::create_dir(dataset.join(".kat-dataset")).unwrap(),
            _ => {}
        }

        let failure = error(inspect_dataset(&dataset));

        assert!(matches!(
            failure,
            DatasetInspectionError::InspectMarker { .. }
                | DatasetInspectionError::InvalidMarker { .. }
        ));
    }
}

#[test]
fn reserved_tables_path_must_be_an_ordinary_directory() {
    let temporary = tempfile::tempdir().unwrap();
    let dataset = dataset(temporary.path());
    fs::write(dataset.join("tables"), "not a directory").unwrap();

    assert!(matches!(
        error(inspect_dataset(&dataset)),
        DatasetInspectionError::InvalidTablesDirectory { .. }
    ));
}

#[test]
fn corrupted_managed_tables_fail_in_name_order() {
    let temporary = tempfile::tempdir().unwrap();
    let dataset = dataset(temporary.path());
    fs::create_dir(dataset.join("tables")).unwrap();
    fs::write(dataset.join("tables/zeta.parquet"), "broken").unwrap();
    fs::write(dataset.join("tables/alpha.parquet"), "broken").unwrap();

    assert!(matches!(
        error(inspect_dataset(&dataset)),
        DatasetInspectionError::ReadTableMetadata { name, .. } if name == "alpha"
    ));
}

#[test]
fn duplicate_top_level_columns_are_rejected() {
    let temporary = tempfile::tempdir().unwrap();
    let dataset = dataset(temporary.path());
    parquet(
        &dataset.join("tables/duplicate.parquet"),
        vec![
            Field::new("value", DataType::Int64, false),
            Field::new("value", DataType::Utf8, true),
        ],
    );

    assert!(matches!(
        error(inspect_dataset(&dataset)),
        DatasetInspectionError::DuplicateColumn { table, column }
            if table == "duplicate" && column == "value"
    ));
}
