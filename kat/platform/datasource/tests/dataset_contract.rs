use arrow_array::{Int32Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use flate2::{Compression, write::GzEncoder};
use kat_datasource::{DatasetLocator, DatasetStore, DatasetWriteTarget};
use parquet::file::reader::{FileReader, SerializedFileReader};
use prost::Message;
use serde_json::json;
#[cfg(any(target_os = "linux", target_os = "redox"))]
use std::process::Command;
use std::{
    env,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::tempdir;

const PROFILER_HEADER_SIZE: usize = 1024;
const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
const HIPROFILER_PROTOBUF_BIN: u32 = 0;
const EXPECTED_MAX_DATASET_ROW_GROUP_ROWS: i64 = 65_536;
const DEFAULT_ENV_CHILD: &str = "KAT_RS_DATASET_CONTRACT_DEFAULT_ENV_CHILD";
const DEFAULT_ENV_EXPECTED_PROJECT_DATA_DIR: &str =
    "KAT_RS_DATASET_CONTRACT_EXPECTED_PROJECT_DATA_DIR";

#[test]
fn dataset_store_resolves_default_under_datasets_dir() {
    let root = tempdir().expect("tempdir");
    let store = DatasetStore::from_datasets_dir(root.path());

    let resolved = store
        .resolve(&DatasetLocator::Default)
        .expect("default resolves");

    assert_eq!(resolved.path, root.path().join("default"));
    assert_eq!(resolved.name.as_deref(), Some("default"));
}

#[cfg(any(target_os = "linux", target_os = "redox"))]
#[test]
fn dataset_store_uses_absolute_linux_xdg_data_home() {
    let data_home = tempdir().expect("data home tempdir");
    let home = tempdir().expect("home tempdir");
    let platform_home = tempdir().expect("platform home tempdir");
    let output = Command::new(env::current_exe().expect("current test binary"))
        .env(DEFAULT_ENV_CHILD, "1")
        .env(
            DEFAULT_ENV_EXPECTED_PROJECT_DATA_DIR,
            data_home.path().join("kat-rs"),
        )
        .env("XDG_DATA_HOME", data_home.path())
        .env("HOME", home.path())
        .env(
            "APPDATA",
            platform_home.path().join("AppData").join("Roaming"),
        )
        .env(
            "LOCALAPPDATA",
            platform_home.path().join("AppData").join("Local"),
        )
        .env("USERPROFILE", platform_home.path())
        .arg("--exact")
        .arg("dataset_store_default_from_env_child")
        .output()
        .expect("child test runs");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(any(target_os = "linux", target_os = "redox"))]
#[test]
fn dataset_store_ignores_relative_linux_xdg_data_home() {
    let home = tempdir().expect("home tempdir");
    let platform_home = tempdir().expect("platform home tempdir");
    let output = Command::new(env::current_exe().expect("current test binary"))
        .env(DEFAULT_ENV_CHILD, "1")
        .env(
            DEFAULT_ENV_EXPECTED_PROJECT_DATA_DIR,
            home.path().join(".local").join("share").join("kat-rs"),
        )
        .env("XDG_DATA_HOME", "relative-data")
        .env("HOME", home.path())
        .env(
            "APPDATA",
            platform_home.path().join("AppData").join("Roaming"),
        )
        .env(
            "LOCALAPPDATA",
            platform_home.path().join("AppData").join("Local"),
        )
        .env("USERPROFILE", platform_home.path())
        .arg("--exact")
        .arg("dataset_store_default_from_env_child")
        .output()
        .expect("child test runs");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dataset_store_default_from_env_child() {
    if env::var_os(DEFAULT_ENV_CHILD).is_none() {
        return;
    }

    let project_data_dir = PathBuf::from(
        env::var_os(DEFAULT_ENV_EXPECTED_PROJECT_DATA_DIR)
            .expect("expected project data dir is set"),
    );
    let store = DatasetStore::default_from_env().expect("store resolves");
    let resolved = store
        .resolve(&DatasetLocator::Default)
        .expect("default resolves");

    assert_eq!(
        resolved.path,
        project_data_dir.join("datasets").join("default")
    );
}

#[test]
fn dataset_store_resolves_named_dataset_under_datasets_dir() {
    let root = tempdir().expect("tempdir");
    let store = DatasetStore::from_datasets_dir(root.path());

    let resolved = store
        .resolve(&DatasetLocator::Name("langfuse-prod".to_string()))
        .expect("name resolves");

    assert_eq!(resolved.path, root.path().join("langfuse-prod"));
    assert_eq!(resolved.name.as_deref(), Some("langfuse-prod"));
}

#[test]
fn dataset_store_rejects_invalid_names() {
    let root = tempdir().expect("tempdir");
    let store = DatasetStore::from_datasets_dir(root.path());

    for value in ["", ".", "..", "a/b", "a\\b"] {
        let result = store.resolve(&DatasetLocator::Name(value.to_string()));
        assert!(result.is_err(), "{value:?} should be rejected");
    }
}

#[tokio::test]
async fn dataset_reader_rejects_legacy_version_and_table_metadata_fields() {
    let dir = tempdir().expect("tempdir");
    let dataset_path = dir.path().join("dataset");
    fs::create_dir(&dataset_path).expect("dataset dir is created");
    let json = r#"{
  "formatVersion": 1,
  "tables": [
    {
      "tableId": "direct.hitrace.sched_switch",
      "name": "sched_switch",
      "source": "HITRACE",
      "category": "DIRECT_EVENT",
      "path": "tables/hitrace.sched_switch.parquet",
      "rowCount": 42,
      "columns": [],
      "schemaFingerprint": "xxh64:0000000000000000"
    }
  ]
}"#;
    fs::write(dataset_path.join("catalog.json"), json).expect("catalog is written");

    let error = match kat_datasource::TraceDatasource::from_dataset(&dataset_path).await {
        Ok(_) => panic!("legacy catalog table metadata should be rejected"),
        Err(error) => error,
    };

    let message = format!("{error:#}");
    assert!(
        message.contains("unknown field"),
        "unexpected error: {message}"
    );
}

#[tokio::test]
async fn hitrace_materialize_rejects_existing_target() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");
    let target = root.path().join("default");
    fs::create_dir_all(&target).expect("target exists");

    let error = kat_datasource::materialize_hitrace_dataset(&trace_path, &target)
        .await
        .expect_err("existing target is rejected");

    assert!(error.to_string().contains("already exists"));
}

#[test]
fn managed_hitrace_import_reuses_migrated_tables_and_reports_unknown_content() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("capture.hitrace");
    let target = root.path().join("dataset");
    let mut bytes = profiler_section(vec![
        TestProfilerPluginData {
            name: "z-plugin".to_string(),
            status: 0,
            data: vec![1],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        },
        TestProfilerPluginData {
            name: "a-plugin_config".to_string(),
            status: 0,
            data: vec![2],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        },
        TestProfilerPluginData {
            name: "z-plugin".to_string(),
            status: 0,
            data: vec![3],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        },
    ]);
    bytes.extend(profiler_section_body(1000, Vec::new()));
    bytes.extend(profiler_section_body(77, Vec::new()));
    fs::write(&trace_path, bytes).expect("trace is written");

    let mut unsupported_content = Vec::new();
    let imported = kat_datasource::import_hitrace(
        &trace_path,
        DatasetWriteTarget::write_to_empty(&target),
        |content| {
            unsupported_content.push((
                content.kind().to_owned(),
                content.value().to_owned(),
                content.byte_offset(),
            ));
            Ok(())
        },
    )
    .expect("Hitrace import succeeds");

    assert_eq!(imported.unsupported_plugins(), ["a-plugin", "z-plugin"]);
    assert_eq!(imported.unsupported_section_types(), [77, 1000]);
    assert_eq!(
        unsupported_content
            .iter()
            .map(|(kind, value, _)| (kind.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        [
            ("plugin", "z-plugin"),
            ("plugin", "a-plugin"),
            ("plugin", "z-plugin"),
            ("section_type", "1000"),
            ("section_type", "77"),
        ]
    );
    assert!(
        unsupported_content
            .windows(2)
            .all(|content| content[0].2 < content[1].2)
    );
    assert!(target.join(".kat-dataset").is_file());
    let tables = kat_datasource::inspect_dataset(&target)
        .expect("managed Dataset can be inspected")
        .tables()
        .iter()
        .map(|table| table.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(tables, ["clock_domain", "clock_snapshot"]);
}

#[test]
fn managed_hitrace_import_streams_repeated_unknown_occurrences_without_retaining_them() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("many-unknown.hitrace");
    let target = root.path().join("dataset");
    let frames = (0..=8192)
        .map(|index| TestProfilerPluginData {
            name: "future-plugin".to_owned(),
            status: 0,
            data: vec![(index % 255) as u8],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        })
        .collect();
    fs::write(&trace_path, profiler_section(frames)).expect("trace is written");

    let mut observed = 0;
    let imported = kat_datasource::import_hitrace(
        &trace_path,
        DatasetWriteTarget::write_to_empty(&target),
        |_| {
            observed += 1;
            Ok(())
        },
    )
    .expect("unknown occurrences remain importable");

    assert_eq!(observed, 8193);
    assert_eq!(imported.unsupported_plugins(), ["future-plugin"]);
}

#[test]
fn managed_hitrace_import_streams_unknown_occurrences_before_decode_failure() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("partially-invalid.hitrace");
    let mut bytes = profiler_section(vec![
        TestProfilerPluginData {
            name: "first-plugin".to_owned(),
            status: 0,
            data: vec![1],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        },
        TestProfilerPluginData {
            name: "second-plugin_config".to_owned(),
            status: 0,
            data: vec![2],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        },
    ]);
    bytes.extend_from_slice(b"truncated-section");
    fs::write(&trace_path, bytes).expect("trace is written");

    let mut observed = Vec::new();
    kat_datasource::import_hitrace(
        &trace_path,
        DatasetWriteTarget::write_to_empty(root.path().join("dataset")),
        |content| {
            observed.push(content.value().to_owned());
            Ok(())
        },
    )
    .expect_err("truncated capture is rejected");

    assert_eq!(observed, ["first-plugin", "second-plugin"]);
}

#[test]
fn unsupported_content_observer_failure_precedes_authorized_target_mutation() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("capture.hitrace");
    let target = root.path().join("dataset");
    fs::write(
        &trace_path,
        profiler_section(vec![TestProfilerPluginData {
            name: "future-plugin".to_owned(),
            status: 0,
            data: vec![1],
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: String::new(),
            sample_interval: 0,
        }]),
    )
    .expect("trace is written");
    fs::create_dir(&target).expect("target exists");
    fs::write(target.join("sentinel"), "unchanged").expect("sentinel is written");

    let error = kat_datasource::import_hitrace(
        &trace_path,
        DatasetWriteTarget::permanently_replace_all_contents(&target),
        |_| Err(io::Error::new(io::ErrorKind::WriteZero, "log is full")),
    )
    .expect_err("observer failure rejects the import");

    assert!(matches!(
        error,
        kat_datasource::HitraceImportError::ObserveUnsupportedContent { .. }
    ));
    assert_eq!(
        fs::read_to_string(target.join("sentinel")).expect("sentinel remains readable"),
        "unchanged"
    );
}

#[test]
fn invalid_hitrace_preserves_authorized_overwrite_target() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("invalid.hitrace");
    let target = root.path().join("dataset");
    fs::write(&trace_path, b"not a Hitrace capture").expect("invalid trace is written");
    fs::create_dir(&target).expect("target directory is created");
    fs::write(target.join("sentinel"), "unchanged").expect("sentinel is written");

    kat_datasource::import_hitrace(
        &trace_path,
        DatasetWriteTarget::permanently_replace_all_contents(&target),
        |_| Ok(()),
    )
    .expect_err("invalid Hitrace is rejected");

    assert_eq!(
        fs::read_to_string(target.join("sentinel")).expect("sentinel remains readable"),
        "unchanged"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn hitrace_materialize_rejects_broken_symlink_target() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");
    let target = root.path().join("default");
    std::os::unix::fs::symlink(root.path().join("missing"), &target)
        .expect("broken symlink target is created");

    let error = kat_datasource::materialize_hitrace_dataset(&trace_path, &target)
        .await
        .expect_err("broken symlink target is rejected");

    assert!(error.to_string().contains("already exists"));
}

#[tokio::test]
async fn hitrace_materialize_rejects_invalid_target_paths() {
    let root = tempdir().expect("tempdir");
    let trace_path = root.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");
    let invalid_targets: Vec<PathBuf> = vec![
        Path::new("").to_path_buf(),
        Path::new(".").to_path_buf(),
        Path::new("..").to_path_buf(),
        Path::new("/").to_path_buf(),
        Path::new("default").to_path_buf(),
        root.path().join(".."),
    ];

    for target in invalid_targets {
        let error = kat_datasource::materialize_hitrace_dataset(&trace_path, &target)
            .await
            .expect_err("invalid target is rejected");

        assert!(
            error.to_string().contains("invalid dataset target path"),
            "unexpected error for {}: {error}",
            target.display()
        );
    }
}

#[tokio::test]
async fn hitrace_dataset_queries_after_source_file_is_removed() {
    let dir = tempdir().expect("tempdir");
    let trace_path = dir.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");
    let dataset_path = dir.path().join("dataset");

    kat_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    fs::remove_file(&trace_path).expect("source is removed");

    let datasource = kat_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let rows = datasource
        .query_json("select prev_comm, prev_pid, next_comm, next_pid from sched_switch")
        .await
        .expect("dataset query works");

    assert_eq!(
        rows,
        json!([{
            "prev_comm": "render",
            "prev_pid": 42,
            "next_comm": "main",
            "next_pid": 7,
        }])
    );
}

#[tokio::test]
async fn materialized_catalog_records_source_table_kind() {
    let dir = tempdir().expect("tempdir");
    let trace_path = dir.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");
    let dataset_path = dir.path().join("dataset");

    kat_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    assert!(!dataset_path.join("manifest.json").exists());

    let catalog: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(dataset_path.join("catalog.json")).expect("catalog is read"),
    )
    .expect("catalog json parses");

    assert_eq!(
        catalog
            .as_object()
            .expect("catalog is an object")
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["tables".to_string()]
    );

    let sched_switch = catalog["tables"]
        .as_array()
        .expect("catalog tables is an array")
        .iter()
        .find(|table| table["name"] == "sched_switch")
        .expect("sched_switch table exists");

    assert_eq!(sched_switch["path"], "tables/hitrace.sched_switch.parquet");
    assert_eq!(sched_switch["kind"], "source");
    assert!(sched_switch["producer"].is_null(), "{sched_switch:?}");
    assert!(catalog["version"].is_null(), "{catalog:?}");
    assert_eq!(
        sched_switch
            .as_object()
            .expect("catalog table is an object")
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["kind".to_string(), "name".to_string(), "path".to_string()]
    );
}

#[tokio::test]
async fn dataset_reader_rejects_source_table_with_producer() {
    let dir = tempdir().expect("tempdir");
    let trace_path = dir.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");
    let dataset_path = dir.path().join("dataset");

    kat_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    fs::write(
        dataset_path.join("catalog.json"),
        r#"{
  "tables": [
    {
      "name": "sched_switch",
      "path": "tables/hitrace.sched_switch.parquet",
      "kind": "source",
      "producer": {
        "packRef": "openharmony-core-test",
        "transformId": "thread_state_segments"
      }
    }
  ]
}"#,
    )
    .expect("catalog is overwritten");

    let error = match kat_datasource::TraceDatasource::from_dataset(&dataset_path).await {
        Ok(_) => panic!("source table with producer should be rejected"),
        Err(error) => error,
    };

    let message = format!("{error:#}");
    assert!(
        message.contains("source dataset table sched_switch must not declare producer"),
        "unexpected error: {message}"
    );
}

#[tokio::test]
async fn dataset_reader_rejects_derived_table_without_producer() {
    let dir = tempdir().expect("tempdir");
    let trace_path = dir.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");
    let dataset_path = dir.path().join("dataset");

    kat_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    fs::write(
        dataset_path.join("catalog.json"),
        r#"{
  "tables": [
    {
      "name": "derived_sched_switch",
      "path": "tables/hitrace.sched_switch.parquet",
      "kind": "derived"
    }
  ]
}"#,
    )
    .expect("catalog is overwritten");

    let error = match kat_datasource::TraceDatasource::from_dataset(&dataset_path).await {
        Ok(_) => panic!("derived table without producer should be rejected"),
        Err(error) => error,
    };

    let message = format!("{error:#}");
    assert!(
        message.contains("derived dataset table derived_sched_switch must declare producer"),
        "unexpected error: {message}"
    );
}

#[tokio::test]
async fn dataset_reader_rejects_unknown_table_kind() {
    let dir = tempdir().expect("tempdir");
    let trace_path = dir.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");
    let dataset_path = dir.path().join("dataset");

    kat_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    fs::write(
        dataset_path.join("catalog.json"),
        r#"{
  "tables": [
    {
      "name": "sched_switch",
      "path": "tables/hitrace.sched_switch.parquet",
      "kind": "temporary"
    }
  ]
}"#,
    )
    .expect("catalog is overwritten");

    let error = match kat_datasource::TraceDatasource::from_dataset(&dataset_path).await {
        Ok(_) => panic!("unknown table kind should be rejected"),
        Err(error) => error,
    };

    let message = format!("{error:#}");
    assert!(
        message.contains("unknown variant") && message.contains("temporary"),
        "unexpected error: {message}"
    );
}

#[tokio::test]
async fn langfuse_dataset_queries_after_source_files_are_removed() {
    let dir = tempdir().expect("tempdir");
    let observations_path = dir.path().join("observations.jsonl.gz");
    let traces_path = dir.path().join("traces.jsonl.gz");
    let dataset_path = dir.path().join("dataset");

    write_jsonl_gz(
        &observations_path,
        &[r#"{"id":"obs-1","trace_id":"trace-1","type":"GENERATION"}"#],
    );
    write_jsonl_gz(&traces_path, &[r#"{"id":"trace-1","name":"chat request"}"#]);

    kat_datasource::materialize_langfuse_legacy_dataset(
        &observations_path,
        &traces_path,
        &dataset_path,
    )
    .await
    .expect("dataset is materialized");

    fs::remove_file(&observations_path).expect("observations source is removed");
    fs::remove_file(&traces_path).expect("traces source is removed");

    let datasource = kat_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let rows = datasource
        .query_json(
            "select o.id, t.name as trace_name \
             from langfuse_observations o \
             join langfuse_traces t on o.trace_id = t.id",
        )
        .await
        .expect("dataset query works");

    assert_eq!(
        rows,
        json!([{ "id": "obs-1", "trace_name": "chat request" }])
    );
}

#[tokio::test]
async fn langfuse_dataset_writes_empty_object_columns_as_json_strings() {
    let dir = tempdir().expect("tempdir");
    let observations_path = dir.path().join("observations.jsonl.gz");
    let traces_path = dir.path().join("traces.jsonl.gz");
    let dataset_path = dir.path().join("dataset");

    write_jsonl_gz(
        &observations_path,
        &[r#"{"id":"obs-1","trace_id":"trace-1","tool_definitions":{}}"#],
    );
    write_jsonl_gz(&traces_path, &[r#"{"id":"trace-1","name":"chat request"}"#]);

    kat_datasource::materialize_langfuse_legacy_dataset(
        &observations_path,
        &traces_path,
        &dataset_path,
    )
    .await
    .expect("dataset is materialized");

    let datasource = kat_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let rows = datasource
        .query_json("select id, tool_definitions from langfuse_observations")
        .await
        .expect("dataset query works");

    assert_eq!(rows, json!([{ "id": "obs-1", "tool_definitions": "{}" }]));
}

#[tokio::test]
async fn derived_table_writer_adds_queryable_catalog_entry() {
    let dir = tempdir().expect("tempdir");
    let trace_path = dir.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");
    let dataset_path = dir.path().join("dataset");

    kat_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    let batch = derived_thread_batch();

    kat_datasource::write_derived_dataset_table(
        &dataset_path,
        "derived_sched_threads",
        "openharmony-core-test",
        "thread_state_segments",
        &[batch],
    )
    .await
    .expect("derived table is written");

    let catalog: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(dataset_path.join("catalog.json")).expect("catalog is read"),
    )
    .expect("catalog json parses");
    let derived = catalog["tables"]
        .as_array()
        .expect("catalog tables is an array")
        .iter()
        .find(|table| table["name"] == "derived_sched_threads")
        .expect("derived table exists");

    assert_eq!(derived["kind"], "derived");
    assert_eq!(
        derived["path"],
        "derived/openharmony-core-test/thread_state_segments.derived_sched_threads.parquet"
    );
    assert_eq!(derived["producer"]["packRef"], "openharmony-core-test");
    assert_eq!(derived["producer"]["transformId"], "thread_state_segments");

    let datasource = kat_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let rows = datasource
        .query_json("select prev_pid, label from derived_sched_threads")
        .await
        .expect("derived table query works");

    assert_eq!(rows, json!([{ "prev_pid": 42, "label": "render-thread" }]));
}

#[tokio::test]
async fn derived_table_writer_rejects_duplicate_table_name() {
    let dir = tempdir().expect("tempdir");
    let trace_path = dir.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");
    let dataset_path = dir.path().join("dataset");

    kat_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    let batch = derived_thread_batch();
    kat_datasource::write_derived_dataset_table(
        &dataset_path,
        "derived_sched_threads",
        "openharmony-core-test",
        "thread_state_segments",
        std::slice::from_ref(&batch),
    )
    .await
    .expect("derived table is written");

    let error = kat_datasource::write_derived_dataset_table(
        &dataset_path,
        "derived_sched_threads",
        "openharmony-core-test",
        "thread_state_segments",
        &[batch],
    )
    .await
    .expect_err("duplicate derived table name is rejected");

    assert!(
        error.to_string().contains("dataset table already exists"),
        "unexpected error: {error:#}"
    );
}

#[tokio::test]
async fn derived_table_writer_rejects_path_unsafe_ids() {
    let dir = tempdir().expect("tempdir");
    let trace_path = dir.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");
    let dataset_path = dir.path().join("dataset");

    kat_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    for (logical_name, pack_ref, transform_id, expected) in [
        (
            "derived_sched_threads",
            "../pack",
            "thread_state_segments",
            "packRef must be a single path component",
        ),
        (
            "derived_sched_threads",
            "openharmony-core-test",
            "thread/state",
            "transformId must be a single path component",
        ),
        (
            "../derived_sched_threads",
            "openharmony-core-test",
            "thread_state_segments",
            "derived table name must be a single path component",
        ),
        (
            "",
            "openharmony-core-test",
            "thread_state_segments",
            "derived table name must not be empty",
        ),
    ] {
        let batch = derived_thread_batch();
        let error = kat_datasource::write_derived_dataset_table(
            &dataset_path,
            logical_name,
            pack_ref,
            transform_id,
            &[batch],
        )
        .await
        .expect_err("path unsafe derived identifiers are rejected");

        let message = format!("{error:#}");
        assert!(
            message.contains(expected),
            "unexpected error for {logical_name:?}, {pack_ref:?}, {transform_id:?}: {message}"
        );
    }
}

#[tokio::test]
async fn langfuse_dataset_splits_large_tables_into_bounded_row_groups() {
    let dir = tempdir().expect("tempdir");
    let observations_path = dir.path().join("observations.jsonl.gz");
    let traces_path = dir.path().join("traces.jsonl.gz");
    let dataset_path = dir.path().join("dataset");

    write_jsonl_gz_generated(
        &observations_path,
        (EXPECTED_MAX_DATASET_ROW_GROUP_ROWS + 1) as usize,
        |row| format!(r#"{{"id":"obs-{row}","trace_id":"trace-1","type":"SPAN"}}"#),
    );
    write_jsonl_gz(&traces_path, &[r#"{"id":"trace-1","name":"chat request"}"#]);

    kat_datasource::materialize_langfuse_legacy_dataset(
        &observations_path,
        &traces_path,
        &dataset_path,
    )
    .await
    .expect("dataset is materialized");

    let parquet_path = dataset_path
        .join("tables")
        .join("langfuse.langfuse_observations.parquet");
    let file = File::open(parquet_path).expect("observations parquet is opened");
    let reader = SerializedFileReader::new(file).expect("observations parquet metadata is read");
    let metadata = reader.metadata();

    assert_eq!(metadata.num_row_groups(), 2);
    assert!(
        metadata.row_group(0).num_rows() <= EXPECTED_MAX_DATASET_ROW_GROUP_ROWS,
        "first row group exceeds bounded row count"
    );
    assert!(
        metadata.row_group(1).num_rows() <= EXPECTED_MAX_DATASET_ROW_GROUP_ROWS,
        "second row group exceeds bounded row count"
    );
}

#[derive(Clone, PartialEq, Message)]
struct TestProfilerPluginData {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(uint32, tag = "2")]
    status: u32,
    #[prost(bytes = "vec", tag = "3")]
    data: Vec<u8>,
    #[prost(int32, tag = "4")]
    clock_id: i32,
    #[prost(uint64, tag = "5")]
    tv_sec: u64,
    #[prost(uint64, tag = "6")]
    tv_nsec: u64,
    #[prost(string, tag = "7")]
    version: String,
    #[prost(uint32, tag = "8")]
    sample_interval: u32,
}

#[derive(Clone, PartialEq, Message)]
struct TestTracePluginResult {
    #[prost(message, repeated, tag = "2")]
    ftrace_cpu_detail: Vec<TestFtraceCpuDetailMsg>,
}

#[derive(Clone, PartialEq, Message)]
struct TestFtraceCpuDetailMsg {
    #[prost(uint32, tag = "1")]
    cpu: u32,
    #[prost(message, repeated, tag = "2")]
    event: Vec<TestFtraceEvent>,
    #[prost(uint64, tag = "3")]
    overwrite: u64,
}

#[derive(Clone, PartialEq, Message)]
struct TestFtraceEvent {
    #[prost(uint64, tag = "1")]
    timestamp: u64,
    #[prost(int32, tag = "2")]
    tgid: i32,
    #[prost(string, tag = "3")]
    comm: String,
    #[prost(message, optional, tag = "2417")]
    sched_switch_format: Option<TestSchedSwitchFormat>,
}

#[derive(Clone, PartialEq, Message)]
struct TestSchedSwitchFormat {
    #[prost(string, tag = "1")]
    prev_comm: String,
    #[prost(int32, tag = "2")]
    prev_pid: i32,
    #[prost(int32, tag = "3")]
    prev_prio: i32,
    #[prost(uint64, tag = "4")]
    prev_state: u64,
    #[prost(string, tag = "5")]
    next_comm: String,
    #[prost(int32, tag = "6")]
    next_pid: i32,
    #[prost(int32, tag = "7")]
    next_prio: i32,
}

fn encoded_trace() -> Vec<u8> {
    let payload = TestTracePluginResult {
        ftrace_cpu_detail: vec![TestFtraceCpuDetailMsg {
            cpu: 2,
            event: vec![TestFtraceEvent {
                timestamp: 10,
                tgid: 500,
                comm: "switch_source".to_string(),
                sched_switch_format: Some(TestSchedSwitchFormat {
                    prev_comm: "render".to_string(),
                    prev_pid: 42,
                    prev_prio: 120,
                    prev_state: 1,
                    next_comm: "main".to_string(),
                    next_pid: 7,
                    next_prio: 100,
                }),
            }],
            overwrite: 0,
        }],
    }
    .encode_to_vec();
    let plugin = TestProfilerPluginData {
        name: "ftrace-plugin".to_string(),
        status: 0,
        data: payload,
        clock_id: 2,
        tv_sec: 10,
        tv_nsec: 100,
        version: "1.0".to_string(),
        sample_interval: 8,
    };
    profiler_section(vec![plugin])
}

fn profiler_section(plugins: Vec<TestProfilerPluginData>) -> Vec<u8> {
    let mut body = Vec::new();
    for plugin in plugins {
        append_segment(&mut body, plugin);
    }

    profiler_section_body(HIPROFILER_PROTOBUF_BIN, body)
}

fn profiler_section_body(data_type: u32, body: Vec<u8>) -> Vec<u8> {
    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((PROFILER_HEADER_SIZE + body.len()) as u64).to_le_bytes());
    bytes[56..60].copy_from_slice(&data_type.to_le_bytes());
    bytes.extend_from_slice(&body);
    bytes
}

fn append_segment(bytes: &mut Vec<u8>, plugin: TestProfilerPluginData) {
    let segment = plugin.encode_to_vec();
    bytes.extend_from_slice(&(segment.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&segment);
}

fn write_jsonl_gz(path: &Path, lines: &[&str]) {
    let file = File::create(path).expect("gzip fixture file is created");
    let mut encoder = GzEncoder::new(file, Compression::default());

    for line in lines {
        writeln!(encoder, "{line}").expect("jsonl line is written");
    }

    encoder.finish().expect("gzip stream is finished");
}

fn derived_thread_batch() -> RecordBatch {
    let schema = Arc::new(Schema::new(vec![
        Field::new("prev_pid", DataType::Int32, false),
        Field::new("label", DataType::Utf8, false),
    ]));
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Int32Array::from(vec![42])),
            Arc::new(StringArray::from(vec!["render-thread"])),
        ],
    )
    .expect("derived batch is built")
}

fn write_jsonl_gz_generated(
    path: &Path,
    row_count: usize,
    mut line_for_row: impl FnMut(usize) -> String,
) {
    let file = File::create(path).expect("gzip fixture file is created");
    let mut encoder = GzEncoder::new(file, Compression::default());

    for row in 0..row_count {
        writeln!(encoder, "{}", line_for_row(row)).expect("jsonl line is written");
    }

    encoder.finish().expect("gzip stream is finished");
}
