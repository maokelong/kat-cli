use arrow_array::{Int32Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use flate2::{Compression, write::GzEncoder};
use kat_rs_datasource::{DatasetLocator, DatasetStore};
use parquet::file::reader::{FileReader, SerializedFileReader};
use prost::Message;
use serde_json::json;
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
async fn sqlite_pack_demo_materializer_writes_only_five_source_tables() {
    let dir = tempdir().expect("tempdir");
    let sqlite_path = dir.path().join("pack-demo.db");
    write_pack_demo_sqlite_fixture(&sqlite_path);
    let dataset_path = dir.path().join("dataset");

    kat_rs_datasource::materialize_sqlite_pack_demo_dataset(&sqlite_path, &dataset_path)
        .await
        .expect("sqlite dataset is materialized");

    let catalog: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(dataset_path.join("catalog.json")).expect("catalog is read"),
    )
    .expect("catalog json parses");
    let tables = catalog["tables"].as_array().expect("tables is an array");
    let names = tables
        .iter()
        .map(|table| table["name"].as_str().expect("table name").to_owned())
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "process".to_string(),
            "thread".to_string(),
            "callstack".to_string(),
            "thread_state".to_string(),
            "instant".to_string(),
        ]
    );
    for table in tables {
        assert_eq!(table["kind"], "source");
        assert!(table["path"].as_str().expect("path").starts_with("tables/sqlite."));
    }

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let thread_rows = datasource
        .query_json("select itid, name, is_main_thread from thread order by itid")
        .await
        .expect("thread rows query works");
    assert_eq!(
        thread_rows,
        json!([
            { "itid": 405, "name": ".tencent.wechat", "is_main_thread": 1 },
            { "itid": 440, "name": "OS_IPC_1_15359", "is_main_thread": 0 }
        ])
    );

    let rows = datasource
        .query_json(
            "select p.name, t.name as thread_name \
             from process p join thread t on t.ipid = p.ipid \
             where p.name = '.tencent.wechat' and t.is_main_thread = 1",
        )
        .await
        .expect("query works");

    assert_eq!(
        rows,
        json!([{ "name": ".tencent.wechat", "thread_name": ".tencent.wechat" }])
    );
}

#[tokio::test]
async fn sqlite_pack_demo_materializer_exposes_instant_rowid() {
    let dir = tempdir().expect("tempdir");
    let sqlite_path = dir.path().join("pack-demo.db");
    write_pack_demo_sqlite_fixture(&sqlite_path);
    let dataset_path = dir.path().join("dataset");

    kat_rs_datasource::materialize_sqlite_pack_demo_dataset(&sqlite_path, &dataset_path)
        .await
        .expect("sqlite dataset is materialized");

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let rows = datasource
        .query_json("select rowid, name, ref, wakeup_from from instant order by rowid")
        .await
        .expect("rowid query works");

    assert_eq!(
        rows,
        json!([{ "rowid": 1, "name": "sched_wakeup", "ref": 405, "wakeup_from": 440 }])
    );
}

#[tokio::test]
async fn sqlite_pack_demo_materializer_rejects_missing_required_table() {
    let dir = tempdir().expect("tempdir");
    let sqlite_path = dir.path().join("missing-table.db");
    let connection = rusqlite::Connection::open(&sqlite_path).expect("sqlite opens");
    connection
        .execute_batch(
            "create table process(id int, ipid int, pid int, name text);
             create table thread(id int, itid int, tid int, name text, ipid int);
             create table callstack(id int, ts int, dur int, callid int, name text);",
        )
        .expect("partial schema is created");
    drop(connection);

    let error = kat_rs_datasource::materialize_sqlite_pack_demo_dataset(
        &sqlite_path,
        dir.path().join("dataset"),
    )
    .await
    .expect_err("missing table is rejected");

    let message = format!("{error:#}");
    assert!(
        message.contains("missing required SQLite table thread_state"),
        "unexpected error: {message}"
    );
}

#[tokio::test]
async fn sqlite_pack_demo_materializer_parses_numeric_text_values() {
    let dir = tempdir().expect("tempdir");
    let sqlite_path = dir.path().join("numeric-text.db");
    write_pack_demo_sqlite_numeric_text_fixture(&sqlite_path);
    let dataset_path = dir.path().join("dataset");

    kat_rs_datasource::materialize_sqlite_pack_demo_dataset(&sqlite_path, &dataset_path)
        .await
        .expect("sqlite dataset is materialized");

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let process_rows = datasource
        .query_json("select id, ipid, start_ts from process")
        .await
        .expect("process numeric text query works");
    let thread_state_rows = datasource
        .query_json("select ts, dur, cpu from thread_state")
        .await
        .expect("thread_state numeric text query works");
    let instant_rows = datasource
        .query_json("select ts, ref, value from instant")
        .await
        .expect("instant numeric text query works");

    assert_eq!(
        process_rows,
        json!([{ "id": 1, "ipid": 89, "start_ts": 123 }])
    );
    assert_eq!(
        thread_state_rows,
        json!([{ "ts": 1100, "dur": 250, "cpu": 0 }])
    );
    assert_eq!(
        instant_rows,
        json!([{ "ts": 1150, "ref": 405, "value": 3.14 }])
    );
}

#[tokio::test]
async fn sqlite_pack_demo_materializer_rejects_unparseable_numeric_text_with_column_name() {
    let dir = tempdir().expect("tempdir");
    let sqlite_path = dir.path().join("numeric-text-invalid.db");
    write_pack_demo_sqlite_invalid_numeric_text_fixture(&sqlite_path);

    let error = kat_rs_datasource::materialize_sqlite_pack_demo_dataset(
        &sqlite_path,
        dir.path().join("dataset"),
    )
    .await
    .expect_err("invalid numeric text is rejected");

    let message = format!("{error:#}");
    assert!(
        message.contains("thread_state.dur"),
        "unexpected error: {message}"
    );
}

#[tokio::test]
async fn dataset_reader_rejects_source_table_with_producer() {
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

    let error = match kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path).await {
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

    kat_rs_datasource::materialize_hitrace_dataset(&trace_path, &dataset_path)
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

    let error = match kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path).await {
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
      "kind": "temporary"
    }
  ]
}"#,
    )
    .expect("catalog is overwritten");

    let error = match kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path).await {
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

    assert_eq!(derived["kind"], "derived");
    assert_eq!(
        derived["path"],
        "derived/openharmony-core-test/thread_state_segments.derived_sched_threads.parquet"
    );
    assert_eq!(derived["producer"]["packRef"], "openharmony-core-test");
    assert_eq!(derived["producer"]["transformId"], "thread_state_segments");

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

fn write_pack_demo_sqlite_fixture(path: &Path) {
    let connection = rusqlite::Connection::open(path).expect("sqlite opens");
    connection
        .execute_batch(
            "create table process(id int, ipid int, pid int, name text, start_ts int);
             create table thread(id int, itid int, tid int, name text, start_ts int, end_ts int, ipid int, is_main_thread int);
             create table callstack(id int, ts int, dur int, callid int, cat text, name text, depth int, parent_id int);
             create table thread_state(id int, ts int, dur int, cpu int, itid int, tid int, pid int, state text);
             create table instant(ts int, name text, ref int, wakeup_from int, ref_type text, value real);
             insert into process(id, ipid, pid, name, start_ts) values (1, 89, 15040, '.tencent.wechat', 0);
             insert into thread(id, itid, tid, name, start_ts, end_ts, ipid, is_main_thread) values (1, 405, 15040, '.tencent.wechat', 0, 0, 89, 1);
             insert into thread(id, itid, tid, name, start_ts, end_ts, ipid, is_main_thread) values (2, 440, 15359, 'OS_IPC_1_15359', 0, 0, 89, 0);
             insert into callstack(id, ts, dur, callid, cat, name, depth, parent_id) values (1, 1000, 100, 405, 'H', 'HandleLaunchAbility##com.tencent.wechat', 0, null);
             insert into callstack(id, ts, dur, callid, cat, name, depth, parent_id) values (2, 1300, 1, 405, 'H', 'UIVsyncTask[firstDrawFrame:1]', 0, null);
             insert into thread_state(id, ts, dur, cpu, itid, tid, pid, state) values (1, 1100, 100, 0, 405, 15040, 15040, 'Sleeping');
             insert into instant(ts, name, ref, wakeup_from, ref_type, value) values (1150, 'sched_wakeup', 405, 440, 'itid', null);",
        )
        .expect("sqlite fixture is written");
}

fn write_pack_demo_sqlite_numeric_text_fixture(path: &Path) {
    let connection = rusqlite::Connection::open(path).expect("sqlite opens");
    connection
        .execute_batch(
            "create table process(id text, ipid text, pid text, name text, start_ts text);
             create table thread(id text, itid text, tid text, name text, start_ts text, end_ts text, ipid text, is_main_thread text);
             create table callstack(id text, ts text, dur text, callid text, cat text, name text, depth text, parent_id text);
             create table thread_state(id text, ts text, dur text, cpu text, itid text, tid text, pid text, state text);
             create table instant(ts text, name text, ref text, wakeup_from text, ref_type text, value text);
             insert into process(id, ipid, pid, name, start_ts) values (1, 89, 15040, '.tencent.wechat', '123');
             insert into thread(id, itid, tid, name, start_ts, end_ts, ipid, is_main_thread) values (1, 405, 15040, '.tencent.wechat', 0, 0, 89, 1);
             insert into callstack(id, ts, dur, callid, cat, name, depth, parent_id) values (1, 1000, 100, 405, 'H', 'HandleLaunchAbility##com.tencent.wechat', 0, null);
             insert into thread_state(id, ts, dur, cpu, itid, tid, pid, state) values (1, '1100', '250', '0', 405, 15040, 15040, 'Sleeping');
             insert into instant(ts, name, ref, wakeup_from, ref_type, value) values ('1150', 'sched_wakeup', '405', 440, 'itid', '3.14');",
        )
        .expect("sqlite numeric text fixture is written");
    rewrite_pack_demo_sqlite_declared_types(
        &connection,
        &[
            (
                "process",
                "create table process(id int, ipid int, pid int, name text, start_ts int)",
            ),
            (
                "thread",
                "create table thread(id int, itid int, tid int, name text, start_ts int, end_ts int, ipid int, is_main_thread int)",
            ),
            (
                "callstack",
                "create table callstack(id int, ts int, dur int, callid int, cat text, name text, depth int, parent_id int)",
            ),
            (
                "thread_state",
                "create table thread_state(id int, ts int, dur int, cpu int, itid int, tid int, pid int, state text)",
            ),
            (
                "instant",
                "create table instant(ts int, name text, ref int, wakeup_from int, ref_type text, value real)",
            ),
        ],
    );
}

fn write_pack_demo_sqlite_invalid_numeric_text_fixture(path: &Path) {
    let connection = rusqlite::Connection::open(path).expect("sqlite opens");
    connection
        .execute_batch(
            "create table process(id text, ipid text, pid text, name text, start_ts text);
             create table thread(id text, itid text, tid text, name text, start_ts text, end_ts text, ipid text, is_main_thread text);
             create table callstack(id text, ts text, dur text, callid text, cat text, name text, depth text, parent_id text);
             create table thread_state(id text, ts text, dur text, cpu text, itid text, tid text, pid text, state text);
             create table instant(ts text, name text, ref text, wakeup_from text, ref_type text, value text);
             insert into process(id, ipid, pid, name, start_ts) values (1, 89, 15040, '.tencent.wechat', 0);
             insert into thread(id, itid, tid, name, start_ts, end_ts, ipid, is_main_thread) values (1, 405, 15040, '.tencent.wechat', 0, 0, 89, 1);
             insert into callstack(id, ts, dur, callid, cat, name, depth, parent_id) values (1, 1000, 100, 405, 'H', 'HandleLaunchAbility##com.tencent.wechat', 0, null);
             insert into thread_state(id, ts, dur, cpu, itid, tid, pid, state) values (1, 1100, 'not-a-number', 0, 405, 15040, 15040, 'Sleeping');
             insert into instant(ts, name, ref, wakeup_from, ref_type, value) values (1150, 'sched_wakeup', 405, 440, 'itid', null);",
        )
        .expect("sqlite invalid numeric text fixture is written");
    rewrite_pack_demo_sqlite_declared_types(
        &connection,
        &[
            (
                "process",
                "create table process(id int, ipid int, pid int, name text, start_ts int)",
            ),
            (
                "thread",
                "create table thread(id int, itid int, tid int, name text, start_ts int, end_ts int, ipid int, is_main_thread int)",
            ),
            (
                "callstack",
                "create table callstack(id int, ts int, dur int, callid int, cat text, name text, depth int, parent_id int)",
            ),
            (
                "thread_state",
                "create table thread_state(id int, ts int, dur int, cpu int, itid int, tid int, pid int, state text)",
            ),
            (
                "instant",
                "create table instant(ts int, name text, ref int, wakeup_from int, ref_type text, value real)",
            ),
        ],
    );
}

fn rewrite_pack_demo_sqlite_declared_types(
    connection: &rusqlite::Connection,
    table_sqls: &[(&str, &str)],
) {
    connection
        .execute_batch("pragma writable_schema=on;")
        .expect("writable schema is enabled");
    for (table, sql) in table_sqls {
        connection
            .execute(
                "update sqlite_master set sql = ?1 where type = 'table' and name = ?2",
                rusqlite::params![sql, table],
            )
            .expect("table schema sql is rewritten");
    }
    connection
        .execute_batch("pragma writable_schema=off;")
        .expect("writable schema is disabled");

    let schema_version: i64 = connection
        .query_row("pragma schema_version", [], |row| row.get(0))
        .expect("schema version is read");
    connection
        .execute_batch(&format!("pragma schema_version={};", schema_version + 1))
        .expect("schema version is bumped");
}
