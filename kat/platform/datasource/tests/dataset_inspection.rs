use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow_schema::{DataType, Field, Schema};
use kat_datasource::{
    DatasetBindingKind, DatasetInspectionError, DatasetMutationError,
    MaterializedSourcePublication, ResolvedSource, SourceInspection, inspect_dataset,
    inspect_dataset_target, publish_materialized_source, resolve_dataset, write_external_binding,
};
use parquet::arrow::ArrowWriter;
use serde_json::json;

fn create_dataset(root: &Path, bindings: serde_json::Value) -> PathBuf {
    let path = root.join("dataset");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join(".kat-dataset"), []).unwrap();
    fs::write(
        path.join("bindings.json"),
        serde_json::to_vec(&json!({ "bindings": bindings })).unwrap(),
    )
    .unwrap();
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

#[test]
fn inspection_and_runtime_resolution_are_source_isolated_and_sorted() {
    let temporary = tempfile::tempdir().unwrap();
    let working_directory = dunce::canonicalize(temporary.path()).unwrap();
    let dataset = create_dataset(
        temporary.path(),
        json!([
            {
                "pack": "zeta-pack",
                "source": "remote_facts",
                "kind": "external",
                "arguments": ["--database", "postgres://example"],
                "working_directory": working_directory,
            },
            {
                "pack": "alpha-pack",
                "source": "facts",
                "kind": "materialized",
                "arguments": ["--capture", "recording.bin"],
                "working_directory": working_directory,
                "tables": ["zeta", "events"],
            }
        ]),
    );
    parquet(
        &dataset.join("sources/alpha-pack/facts/tables/zeta.parquet"),
        vec![Field::new("value", DataType::UInt64, false)],
    );
    parquet(
        &dataset.join("sources/alpha-pack/facts/tables/events.parquet"),
        vec![
            Field::new("ts", DataType::Int64, false),
            Field::new("label", DataType::Utf8, true),
        ],
    );

    let inspection = inspect_dataset(&dataset).unwrap();
    assert_eq!(inspection.path(), dunce::canonicalize(&dataset).unwrap());
    assert_eq!(inspection.sources().len(), 2);
    assert_eq!(inspection.sources()[0].pack(), "alpha-pack");
    assert_eq!(inspection.sources()[0].source(), "facts");
    let tables = inspection.sources()[0].tables().unwrap();
    assert_eq!(
        tables.iter().map(|table| table.name()).collect::<Vec<_>>(),
        ["events", "zeta"]
    );
    assert_eq!(tables[0].columns()[0].name(), "ts");
    assert_eq!(tables[0].columns()[0].data_type(), "Int64");
    assert!(!tables[0].columns()[0].nullable());
    assert_eq!(inspection.sources()[1].kind(), DatasetBindingKind::External);
    assert!(inspection.sources()[1].tables().is_none());

    let resolved = resolve_dataset(&dataset).unwrap();
    assert_eq!(resolved.sources().len(), 2);
    let ResolvedSource::Materialized { tables, .. } = &resolved.sources()[0] else {
        panic!("first Source is materialized")
    };
    assert_eq!(tables[0].name(), "events");
    assert_eq!(
        resolved.sources()[0].arguments().unwrap(),
        ["--capture", "recording.bin"]
    );
    assert_eq!(
        tables[0].path(),
        dunce::canonicalize(dataset.join("sources/alpha-pack/facts/tables/events.parquet"))
            .unwrap()
    );
    let runtime_json = serde_json::to_value(&resolved).unwrap();
    assert_eq!(runtime_json["sources"][0]["kind"], "materialized");
    assert_eq!(runtime_json["sources"][1]["kind"], "external");
    assert_eq!(
        runtime_json["sources"][1]["arguments"],
        json!(["--database", "postgres://example"])
    );
}

#[test]
fn empty_dataset_is_legal_but_legacy_flat_dataset_is_rejected() {
    let temporary = tempfile::tempdir().unwrap();
    let dataset = create_dataset(temporary.path(), json!([]));
    assert!(inspect_dataset(&dataset).unwrap().sources().is_empty());

    let legacy = temporary.path().join("legacy");
    fs::create_dir_all(legacy.join("tables")).unwrap();
    fs::write(legacy.join(".kat-dataset"), []).unwrap();
    parquet(
        &legacy.join("tables/events.parquet"),
        vec![Field::new("value", DataType::Int64, false)],
    );
    assert!(matches!(
        inspect_dataset(&legacy),
        Err(DatasetInspectionError::LegacyDataset { .. })
    ));
}

#[test]
fn bindings_metadata_is_strict_and_requires_unique_source_identity() {
    let working_directory = dunce::canonicalize(std::env::current_dir().unwrap()).unwrap();
    let cases = [
        json!([{
            "pack": "example-pack",
            "source": "facts",
            "kind": "external",
            "arguments": [],
            "working_directory": "relative",
        }]),
        json!([{
            "pack": "example-pack",
            "source": "facts",
            "kind": "materialized",
            "arguments": [],
            "working_directory": working_directory,
            "tables": [],
        }]),
        json!([
            {
                "pack": "example-pack",
                "source": "facts",
                "kind": "external",
                "arguments": [],
                "working_directory": working_directory,
            },
            {
                "pack": "example-pack",
                "source": "facts",
                "kind": "materialized",
                "arguments": [],
                "working_directory": working_directory,
                "tables": ["events"],
            }
        ]),
        json!([{
            "pack": "example-pack",
            "source": "facts",
            "kind": "external",
            "arguments": [],
            "working_directory": working_directory,
            "version": 1,
        }]),
    ];

    for bindings in cases {
        let temporary = tempfile::tempdir().unwrap();
        let dataset = create_dataset(temporary.path(), bindings);
        assert!(inspect_dataset(&dataset).is_err());
    }
}

#[test]
fn information_schema_is_rejected_as_a_source_identity() {
    let temporary = tempfile::tempdir().unwrap();
    let working_directory = dunce::canonicalize(temporary.path()).unwrap();
    let existing = create_dataset(
        temporary.path(),
        json!([{
            "pack": "example-pack",
            "source": "information_schema",
            "kind": "external",
            "arguments": [],
            "working_directory": working_directory,
        }]),
    );

    assert!(matches!(
        inspect_dataset(&existing),
        Err(DatasetInspectionError::InvalidSourceName { name })
            if name == "information_schema"
    ));

    let new_dataset = temporary.path().join("new-dataset");
    let error = write_external_binding(
        &new_dataset,
        "example-pack",
        "information_schema",
        vec![],
        &working_directory,
        false,
    )
    .expect_err("the reserved Source identity must be rejected");
    let DatasetMutationError::InvalidBinding(DatasetInspectionError::InvalidSourceName { name }) =
        error
    else {
        panic!("unexpected error: {error}");
    };
    assert_eq!(name, "information_schema");
    assert!(!new_dataset.exists());
}

#[test]
fn materialized_table_directory_is_rejected() {
    let temporary = tempfile::tempdir().unwrap();
    let working_directory = dunce::canonicalize(temporary.path()).unwrap();
    let dataset = create_dataset(
        temporary.path(),
        json!([{
            "pack": "example-pack",
            "source": "facts",
            "kind": "materialized",
            "arguments": [],
            "working_directory": working_directory,
            "tables": ["events"],
        }]),
    );
    let fragments = dataset.join("sources/example-pack/facts/tables/events.parquet");
    parquet(
        &fragments.join("b.parquet"),
        vec![Field::new("label", DataType::Utf8, true)],
    );
    parquet(
        &fragments.join("a.parquet"),
        vec![Field::new("ts", DataType::Int64, false)],
    );
    fs::write(fragments.join("README.txt"), "unmanaged residue").unwrap();

    assert!(matches!(
        inspect_dataset(&dataset),
        Err(DatasetInspectionError::InvalidTableStorage { .. })
    ));
}

#[test]
fn external_binding_creation_and_replace_are_explicit() {
    let temporary = tempfile::tempdir().unwrap();
    let dataset = temporary.path().join("new-dataset");
    let working_directory = dunce::canonicalize(temporary.path()).unwrap();

    let target = inspect_dataset_target(&dataset, "example-pack", "facts").unwrap();
    assert!(!target.exists());
    assert_eq!(target.binding(), None);

    write_external_binding(
        &dataset,
        "example-pack",
        "facts",
        vec!["--file".into(), "one.log".into()],
        &working_directory,
        false,
    )
    .unwrap();
    let occupied = inspect_dataset_target(&dataset, "example-pack", "facts").unwrap();
    assert!(occupied.exists());
    assert_eq!(occupied.binding(), Some(DatasetBindingKind::External));
    assert_eq!(
        occupied.resolved_binding().unwrap().arguments().unwrap(),
        ["--file", "one.log"]
    );
    assert!(matches!(
        write_external_binding(
            &dataset,
            "example-pack",
            "facts",
            vec![],
            &working_directory,
            false,
        ),
        Err(DatasetMutationError::BindingExists { .. })
    ));

    let replaced = write_external_binding(
        &dataset,
        "example-pack",
        "facts",
        vec!["--file".into(), "two.log".into()],
        &working_directory,
        true,
    )
    .unwrap();
    assert_eq!(
        replaced.sources()[0].arguments().unwrap(),
        ["--file", "two.log"]
    );

    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(dataset.join("bindings.json")).unwrap()).unwrap();
    assert_eq!(
        metadata["bindings"][0]["arguments"],
        json!(["--file", "two.log"])
    );
}

#[test]
fn materialized_publication_validates_exports_and_replaces_one_source_space() {
    let temporary = tempfile::tempdir().unwrap();
    let working_directory = dunce::canonicalize(temporary.path()).unwrap();
    let dataset = temporary.path().join("dataset");
    let export = temporary.path().join("export");
    parquet(
        &export.join("events.parquet"),
        vec![Field::new("ts", DataType::Int64, false)],
    );
    parquet(
        &export.join("processes.parquet"),
        vec![Field::new("pid", DataType::UInt32, false)],
    );

    let table_names = ["processes".into(), "events".into()];
    let resolved = publish_materialized_source(
        &dataset,
        MaterializedSourcePublication {
            pack: "example-pack",
            source: "facts",
            arguments: vec!["--input".into(), "capture.bin".into()],
            working_directory: &working_directory,
            table_names: &table_names,
            export_directory: &export,
            replace: false,
        },
    )
    .unwrap();
    let tables = resolved.sources()[0].tables().unwrap();
    assert_eq!(
        tables.iter().map(|table| table.name()).collect::<Vec<_>>(),
        ["events", "processes"]
    );
    assert_eq!(
        resolved.sources()[0].arguments().unwrap(),
        ["--input", "capture.bin"]
    );
    assert!(
        dataset
            .join("sources/example-pack/facts/tables/events.parquet")
            .is_file()
    );

    let replacement = temporary.path().join("replacement");
    parquet(
        &replacement.join("events.parquet"),
        vec![Field::new("message", DataType::Utf8, true)],
    );
    publish_materialized_source(
        &dataset,
        MaterializedSourcePublication {
            pack: "example-pack",
            source: "facts",
            arguments: vec!["--input".into(), "replacement.bin".into()],
            working_directory: &working_directory,
            table_names: &["events".into()],
            export_directory: &replacement,
            replace: true,
        },
    )
    .unwrap();
    assert!(
        !dataset
            .join("sources/example-pack/facts/tables/processes.parquet")
            .exists()
    );
    let inspection = inspect_dataset(&dataset).unwrap();
    let SourceInspection::Materialized { tables, .. } = &inspection.sources()[0] else {
        panic!("Source remains materialized")
    };
    assert_eq!(tables[0].columns()[0].name(), "message");
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(dataset.join("bindings.json")).unwrap()).unwrap();
    assert_eq!(
        metadata["bindings"][0]["arguments"],
        json!(["--input", "replacement.bin"])
    );
    assert_eq!(
        metadata["bindings"][0]["working_directory"],
        json!(working_directory)
    );
}

#[test]
fn invalid_export_does_not_create_a_dataset_target() {
    let temporary = tempfile::tempdir().unwrap();
    let working_directory = dunce::canonicalize(temporary.path()).unwrap();
    let dataset = temporary.path().join("dataset");
    let export = temporary.path().join("export");
    fs::create_dir(&export).unwrap();
    fs::write(export.join("events.parquet"), "not parquet").unwrap();

    assert!(
        publish_materialized_source(
            &dataset,
            MaterializedSourcePublication {
                pack: "example-pack",
                source: "facts",
                arguments: vec![],
                working_directory: &working_directory,
                table_names: &["events".into()],
                export_directory: &export,
                replace: false,
            },
        )
        .is_err()
    );
    assert!(!dataset.exists());
}

#[cfg(unix)]
#[test]
fn dataset_root_symlink_resolves_to_the_canonical_target() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().unwrap();
    let target = create_dataset(temporary.path(), json!([]));
    let alias = temporary.path().join("dataset-alias");
    symlink(&target, &alias).unwrap();

    let resolved = resolve_dataset(&alias).unwrap();

    assert_eq!(resolved.path(), dunce::canonicalize(target).unwrap());
}

#[test]
fn an_existing_plain_directory_is_not_adopted_as_a_dataset() {
    let temporary = tempfile::tempdir().unwrap();
    let dataset = temporary.path().join("plain-directory");
    fs::create_dir(&dataset).unwrap();

    assert!(matches!(
        inspect_dataset_target(&dataset, "example-pack", "facts"),
        Err(DatasetMutationError::InspectDataset(_))
    ));
}
