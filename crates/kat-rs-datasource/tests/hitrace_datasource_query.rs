use std::fs;

use kat_rs_datasource::proto::ProfilerPluginData;
use prost::Message;
use serde_json::json;
use tempfile::tempdir;

const PROFILER_HEADER_SIZE: usize = 1024;
const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
const HIPROFILER_PROTOBUF_BIN: u32 = 0;

#[tokio::test]
async fn build_releases_mmap_and_queries_hitrace_as_json() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("sample.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");

    let datasource = kat_rs_datasource::TraceDatasource::build(
        kat_rs_datasource::DataSourceConfig::hitrace(&trace_path),
    )
    .expect("datasource builds");

    fs::remove_file(&trace_path).expect("trace file can be removed after build");

    let rows = datasource
        .query_json(
            "select count(*) as count, max(sample_interval) as max_sample_interval \
             from profiler_plugin_data",
        )
        .await
        .expect("query succeeds");

    assert_eq!(rows, json!([{ "count": 2, "max_sample_interval": 16 }]));

    let data_rows = datasource
        .query_json("select data from profiler_plugin_data where name = 'ftrace-plugin_config'")
        .await
        .expect("binary query succeeds");

    assert_eq!(data_rows, json!([{ "data": "010203" }]));
}

#[test]
fn build_rejects_len_prefixed_segments_without_hitrace_header() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("segment-only.hitrace");
    let mut bytes = Vec::new();
    append_segment(
        &mut bytes,
        ProfilerPluginData {
            name: "ftrace-plugin".to_string(),
            status: 0,
            data: vec![1, 2, 3],
            clock_id: 2,
            tv_sec: 10,
            tv_nsec: 100,
            version: "1.0".to_string(),
            sample_interval: 8,
        },
    );
    fs::write(&trace_path, bytes).expect("trace is written");

    let result = kat_rs_datasource::TraceDatasource::build(
        kat_rs_datasource::DataSourceConfig::hitrace(&trace_path),
    );
    let Err(error) = result else {
        panic!("segment-only input is rejected");
    };

    assert!(
        error.to_string().contains("missing OHOSPROF header"),
        "{error:#}"
    );
}

fn encoded_trace() -> Vec<u8> {
    let mut bytes = profiler_section(vec![ProfilerPluginData {
        name: "ftrace-plugin_config".to_string(),
        status: 0,
        data: vec![1, 2, 3],
        clock_id: 2,
        tv_sec: 10,
        tv_nsec: 100,
        version: "1.0".to_string(),
        sample_interval: 8,
    }]);
    bytes.extend_from_slice(&profiler_section(vec![ProfilerPluginData {
        name: "ftrace-plugin".to_string(),
        status: 1,
        data: vec![4, 5],
        clock_id: 2,
        tv_sec: 10,
        tv_nsec: 200,
        version: "1.0".to_string(),
        sample_interval: 16,
    }]));
    bytes
}

fn profiler_section(plugins: Vec<ProfilerPluginData>) -> Vec<u8> {
    let mut body = Vec::new();
    for plugin in plugins {
        append_segment(&mut body, plugin);
    }

    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((PROFILER_HEADER_SIZE + body.len()) as u64).to_le_bytes());
    bytes[56..60].copy_from_slice(&HIPROFILER_PROTOBUF_BIN.to_le_bytes());
    bytes.extend_from_slice(&body);
    bytes
}

fn append_segment(bytes: &mut Vec<u8>, plugin: ProfilerPluginData) {
    let segment = plugin.encode_to_vec();
    bytes.extend_from_slice(&(segment.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&segment);
}
