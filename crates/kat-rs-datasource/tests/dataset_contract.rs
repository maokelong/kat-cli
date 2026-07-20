use arrow_array::{Int32Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use flate2::{Compression, write::GzEncoder};
use kat_rs_datasource::{DatasetLocator, DatasetStore};
use parquet::file::reader::{FileReader, SerializedFileReader};
use prost::{Message, Oneof};
use serde_json::json;
#[cfg(any(target_os = "linux", target_os = "redox"))]
use std::process::Command;
use std::{
    env,
    fs::{self, File},
    io::Write,
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

    let error = match kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path).await {
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

    let error = kat_rs_datasource::materialize_hitrace_dataset(&trace_path, &target)
        .await
        .expect_err("existing target is rejected");

    assert!(error.to_string().contains("already exists"));
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

    let error = kat_rs_datasource::materialize_hitrace_dataset(&trace_path, &target)
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
        let error = kat_rs_datasource::materialize_hitrace_dataset(&trace_path, &target)
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

    kat_rs_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    fs::remove_file(&trace_path).expect("source is removed");

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let rows = datasource
        .query_json(
            "select prev_comm, prev_pid, next_comm, next_pid \
             from trace_plugin_result__ftrace_cpu_detail__event__sched_switch_format",
        )
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
async fn materialized_catalog_records_only_table_path_and_format() {
    let dir = tempdir().expect("tempdir");
    let trace_path = dir.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");
    let dataset_path = dir.path().join("dataset");

    kat_rs_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
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
        .find(|table| {
            table["name"] == "trace_plugin_result__ftrace_cpu_detail__event__sched_switch_format"
        })
        .expect("sched_switch table exists");

    assert_eq!(
        sched_switch["path"],
        "tables/trace_plugin_result__ftrace_cpu_detail__event__sched_switch_format.parquet"
    );
    assert_eq!(sched_switch["format"], "parquet");
    assert!(catalog["version"].is_null(), "{catalog:?}");
    assert_eq!(
        sched_switch
            .as_object()
            .expect("catalog table is an object")
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["format".to_string(), "name".to_string(), "path".to_string()]
    );
}

#[tokio::test]
async fn hitrace_materialize_writes_relational_tables_for_prototype_rules() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("relational.hitrace");
    fs::write(&trace_path, relational_trace()).expect("trace is written");
    let dataset_path = dir.path().join("dataset");

    kat_rs_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");

    let overview = datasource
        .query_json("select zram, gpu_used_size from memory_data")
        .await
        .expect("memory overview query succeeds");
    assert_eq!(overview, json!([{ "zram": 64u64, "gpu_used_size": 32u64 }]));

    let source_indexes = datasource
        .query_json(
            "select 'memory' as table_name, source_index from memory_data \
             union all \
             select 'ftrace' as table_name, source_index from trace_plugin_result__ftrace_cpu_detail__event__sched_switch_format \
             union all \
             select 'native_hook' as table_name, source_index \
             from batch_native_hook_data__events__alloc_event \
             order by table_name",
        )
        .await
        .expect("source index query succeeds");
    assert_eq!(
        source_indexes,
        json!([
            { "table_name": "ftrace", "source_index": 0u64 },
            { "table_name": "memory", "source_index": 0u64 },
            { "table_name": "native_hook", "source_index": 0u64 },
        ])
    );

    let smaps = datasource
        .query_json(
            "select p.pid, s.path, s.rss \
             from memory_data__processesinfo__smapinfo s \
             join memory_data__processesinfo p \
               on s.source_index = p.source_index \
              and s.parent_index = p.row_index",
        )
        .await
        .expect("memory smaps join query succeeds");
    assert_eq!(
        smaps,
        json!([{ "pid": 42, "path": "/system/lib/libark.so", "rss": 512u64 }])
    );

    let ftrace = datasource
        .query_json(
            "select c.cpu as event_cpu, s.prev_comm, s.next_comm \
             from trace_plugin_result__ftrace_cpu_detail__event__sched_switch_format s \
             join trace_plugin_result__ftrace_cpu_detail__event e \
               on s.source_index = e.source_index \
              and s.parent_index = e.row_index \
             join trace_plugin_result__ftrace_cpu_detail c \
               on e.source_index = c.source_index \
              and e.parent_index = c.row_index",
        )
        .await
        .expect("ftrace event query succeeds");
    assert_eq!(
        ftrace,
        json!([{ "event_cpu": 3, "prev_comm": "RenderThread", "next_comm": "main" }])
    );

    let alloc = datasource
        .query_json(
            "select pid, tid, addr, size \
             from batch_native_hook_data__events__alloc_event",
        )
        .await
        .expect("native hook alloc query succeeds");
    assert_eq!(
        alloc,
        json!([{ "pid": 42, "tid": 43, "addr": 4096u64, "size": 64u64 }])
    );

    let frames = datasource
        .query_json(
            "select symbol_name, file_path \
             from batch_native_hook_data__events__alloc_event__frame_info",
        )
        .await
        .expect("native hook frame query succeeds");
    assert_eq!(
        frames,
        json!([{ "symbol_name": "malloc", "file_path": "/system/lib/libc.so" }])
    );
}

#[tokio::test]
async fn hitrace_streaming_flush_keeps_row_indexes_and_parent_joins() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("streaming-flush.hitrace");
    let event_count = (EXPECTED_MAX_DATASET_ROW_GROUP_ROWS + 3) as usize;
    fs::write(&trace_path, streaming_flush_trace(event_count)).expect("trace is written");
    let dataset_path = dir.path().join("dataset");

    kat_rs_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");

    let event_stats = datasource
        .query_json(
            "select count(*) as row_count, \
                    min(row_index) as min_row_index, \
                    max(row_index) as max_row_index, \
                    count(distinct row_index) as distinct_row_indexes \
             from trace_plugin_result__ftrace_cpu_detail__event",
        )
        .await
        .expect("event row indexes query succeeds");
    assert_eq!(
        event_stats,
        json!([{
            "row_count": event_count as u64,
            "min_row_index": 0u64,
            "max_row_index": (event_count - 1) as u64,
            "distinct_row_indexes": event_count as u64,
        }])
    );

    let joined_rows = datasource
        .query_json(
            "select count(*) as row_count \
             from trace_plugin_result__ftrace_cpu_detail__event__sched_switch_format s \
             join trace_plugin_result__ftrace_cpu_detail__event e \
               on s.source_index = e.source_index \
              and s.parent_index = e.row_index",
        )
        .await
        .expect("flushed child rows still join parent events");
    assert_eq!(joined_rows, json!([{ "row_count": event_count as u64 }]));
}

#[tokio::test]
async fn from_hitrace_registers_relational_tables() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("relational.hitrace");
    fs::write(&trace_path, relational_trace()).expect("trace is written");

    let datasource =
        kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path).expect("datasource builds");

    fs::remove_file(&trace_path).expect("source can be removed after build");

    let overview = datasource
        .query_json("select zram, gpu_used_size from memory_data")
        .await
        .expect("memory overview query succeeds");
    assert_eq!(overview, json!([{ "zram": 64u64, "gpu_used_size": 32u64 }]));

    assert!(
        datasource
            .query_json("select count(*) from process_data_processesinfo")
            .await
            .is_err(),
        "old fixed_result child table should not be registered"
    );
}

#[tokio::test]
async fn dataset_reader_rejects_unsupported_table_format() {
    let dir = tempdir().expect("tempdir");
    let trace_path = dir.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");
    let dataset_path = dir.path().join("dataset");

    kat_rs_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    fs::write(
        dataset_path.join("catalog.json"),
        r#"{
  "tables": [
    {
      "name": "sched_switch",
      "path": "tables/hitrace.sched_switch.parquet",
      "format": "json"
    }
  ]
}"#,
    )
    .expect("catalog is overwritten");

    let error = match kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path).await {
        Ok(_) => panic!("unsupported table format should be rejected"),
        Err(error) => error,
    };

    let message = format!("{error:#}");
    assert!(
        message.contains("dataset table sched_switch has unsupported format: json"),
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

    kat_rs_datasource::materialize_langfuse_legacy_dataset(
        &observations_path,
        &traces_path,
        &dataset_path,
    )
    .await
    .expect("dataset is materialized");

    fs::remove_file(&observations_path).expect("observations source is removed");
    fs::remove_file(&traces_path).expect("traces source is removed");

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
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

    kat_rs_datasource::materialize_langfuse_legacy_dataset(
        &observations_path,
        &traces_path,
        &dataset_path,
    )
    .await
    .expect("dataset is materialized");

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
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

    kat_rs_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    let batch = derived_thread_batch();

    kat_rs_datasource::write_derived_dataset_table(
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

    assert_eq!(
        derived["path"],
        "derived/openharmony-core-test/thread_state_segments.derived_sched_threads.parquet"
    );
    assert_eq!(derived["format"], "parquet");
    assert_eq!(
        derived
            .as_object()
            .expect("derived catalog table is an object")
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["format".to_string(), "name".to_string(), "path".to_string()]
    );

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
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

    kat_rs_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
        .await
        .expect("dataset is materialized");

    let batch = derived_thread_batch();
    kat_rs_datasource::write_derived_dataset_table(
        &dataset_path,
        "derived_sched_threads",
        "openharmony-core-test",
        "thread_state_segments",
        std::slice::from_ref(&batch),
    )
    .await
    .expect("derived table is written");

    let error = kat_rs_datasource::write_derived_dataset_table(
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

    kat_rs_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
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
        let error = kat_rs_datasource::write_derived_dataset_table(
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

    kat_rs_datasource::materialize_langfuse_legacy_dataset(
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

#[derive(Clone, PartialEq, Message)]
struct RelationalMemoryData {
    #[prost(message, repeated, tag = "1")]
    processesinfo: Vec<RelationalProcessMemoryInfo>,
    #[prost(uint64, tag = "4")]
    zram: u64,
    #[prost(uint64, tag = "10")]
    gpu_used_size: u64,
}

#[derive(Clone, PartialEq, Message)]
struct RelationalProcessMemoryInfo {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(string, tag = "2")]
    name: String,
    #[prost(message, repeated, tag = "12")]
    smapinfo: Vec<RelationalSmapsInfo>,
}

#[derive(Clone, PartialEq, Message)]
struct RelationalSmapsInfo {
    #[prost(string, tag = "4")]
    path: String,
    #[prost(uint64, tag = "6")]
    rss: u64,
}

#[derive(Clone, PartialEq, Message)]
struct RelationalBatchNativeHookData {
    #[prost(message, repeated, tag = "1")]
    events: Vec<RelationalNativeHookData>,
}

#[derive(Clone, PartialEq, Message)]
struct RelationalNativeHookData {
    #[prost(uint64, tag = "1")]
    tv_sec: u64,
    #[prost(uint64, tag = "2")]
    tv_nsec: u64,
    #[prost(oneof = "RelationalNativeHookEvent", tags = "3")]
    event: Option<RelationalNativeHookEvent>,
}

#[derive(Clone, PartialEq, Oneof)]
enum RelationalNativeHookEvent {
    #[prost(message, tag = "3")]
    AllocEvent(RelationalAllocEvent),
}

#[derive(Clone, PartialEq, Message)]
struct RelationalAllocEvent {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(int32, tag = "2")]
    tid: i32,
    #[prost(uint64, tag = "3")]
    addr: u64,
    #[prost(uint64, tag = "4")]
    size: u64,
    #[prost(message, repeated, tag = "5")]
    frame_info: Vec<RelationalFrame>,
}

#[derive(Clone, PartialEq, Message)]
struct RelationalFrame {
    #[prost(string, tag = "3")]
    symbol_name: String,
    #[prost(string, tag = "4")]
    file_path: String,
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
    let mut body = Vec::new();
    append_segment(&mut body, plugin);

    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((PROFILER_HEADER_SIZE + body.len()) as u64).to_le_bytes());
    bytes[56..60].copy_from_slice(&HIPROFILER_PROTOBUF_BIN.to_le_bytes());
    bytes.extend_from_slice(&body);
    bytes
}

fn relational_trace() -> Vec<u8> {
    let mut body = Vec::new();
    append_segment(
        &mut body,
        fixed_result_plugin(
            "memory-plugin",
            RelationalMemoryData {
                zram: 64,
                gpu_used_size: 32,
                processesinfo: vec![RelationalProcessMemoryInfo {
                    pid: 42,
                    name: "render".to_string(),
                    smapinfo: vec![RelationalSmapsInfo {
                        path: "/system/lib/libark.so".to_string(),
                        rss: 512,
                    }],
                }],
            },
        ),
    );
    append_segment(
        &mut body,
        TestProfilerPluginData {
            name: "ftrace-plugin".to_string(),
            status: 1,
            data: TestTracePluginResult {
                ftrace_cpu_detail: vec![TestFtraceCpuDetailMsg {
                    cpu: 3,
                    event: vec![TestFtraceEvent {
                        timestamp: 10,
                        tgid: 500,
                        comm: "switch_source".to_string(),
                        sched_switch_format: Some(TestSchedSwitchFormat {
                            prev_comm: "RenderThread".to_string(),
                            prev_pid: 42,
                            prev_prio: 120,
                            prev_state: 1,
                            next_comm: "main".to_string(),
                            next_pid: 100,
                            next_prio: 120,
                        }),
                    }],
                    overwrite: 0,
                }],
            }
            .encode_to_vec(),
            clock_id: 2,
            tv_sec: 10,
            tv_nsec: 200,
            version: "1.0".to_string(),
            sample_interval: 16,
        },
    );
    append_segment(
        &mut body,
        TestProfilerPluginData {
            name: "nativehook".to_string(),
            status: 1,
            data: RelationalBatchNativeHookData {
                events: vec![RelationalNativeHookData {
                    tv_sec: 1,
                    tv_nsec: 20,
                    event: Some(RelationalNativeHookEvent::AllocEvent(
                        RelationalAllocEvent {
                            pid: 42,
                            tid: 43,
                            addr: 0x1000,
                            size: 64,
                            frame_info: vec![RelationalFrame {
                                symbol_name: "malloc".to_string(),
                                file_path: "/system/lib/libc.so".to_string(),
                            }],
                        },
                    )),
                }],
            }
            .encode_to_vec(),
            clock_id: 2,
            tv_sec: 10,
            tv_nsec: 200,
            version: "1.0".to_string(),
            sample_interval: 10,
        },
    );

    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((PROFILER_HEADER_SIZE + body.len()) as u64).to_le_bytes());
    bytes[56..60].copy_from_slice(&HIPROFILER_PROTOBUF_BIN.to_le_bytes());
    bytes.extend_from_slice(&body);
    bytes
}

fn streaming_flush_trace(event_count: usize) -> Vec<u8> {
    let events = (0..event_count)
        .map(|index| TestFtraceEvent {
            timestamp: index as u64,
            tgid: 500,
            comm: "switch_source".to_string(),
            sched_switch_format: Some(TestSchedSwitchFormat {
                prev_comm: "RenderThread".to_string(),
                prev_pid: 42,
                prev_prio: 120,
                prev_state: 1,
                next_comm: "main".to_string(),
                next_pid: 100,
                next_prio: 120,
            }),
        })
        .collect();
    let payload = TestTracePluginResult {
        ftrace_cpu_detail: vec![TestFtraceCpuDetailMsg {
            cpu: 3,
            event: events,
            overwrite: 0,
        }],
    }
    .encode_to_vec();

    let plugin = TestProfilerPluginData {
        name: "ftrace-plugin".to_string(),
        status: 1,
        data: payload,
        clock_id: 2,
        tv_sec: 10,
        tv_nsec: 200,
        version: "1.0".to_string(),
        sample_interval: 16,
    };

    let mut body = Vec::new();
    append_segment(&mut body, plugin);
    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((PROFILER_HEADER_SIZE + body.len()) as u64).to_le_bytes());
    bytes[56..60].copy_from_slice(&HIPROFILER_PROTOBUF_BIN.to_le_bytes());
    bytes.extend_from_slice(&body);
    bytes
}

fn fixed_result_plugin(name: &str, message: impl Message) -> TestProfilerPluginData {
    TestProfilerPluginData {
        name: name.to_string(),
        status: 1,
        data: message.encode_to_vec(),
        clock_id: 2,
        tv_sec: 10,
        tv_nsec: 200,
        version: "1.0".to_string(),
        sample_interval: 16,
    }
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
