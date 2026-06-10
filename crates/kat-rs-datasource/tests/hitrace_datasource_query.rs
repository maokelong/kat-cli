use std::fs;

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

    let datasource =
        kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path).expect("datasource builds");

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
        TestProfilerPluginData {
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

    let result = kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path);
    let Err(error) = result else {
        panic!("segment-only input is rejected");
    };

    assert!(
        error.to_string().contains("missing OHOSPROF header"),
        "{error:#}"
    );
}

#[test]
fn build_rejects_overflowing_section_length_without_panic() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("overflowing-section.hitrace");
    let mut bytes = profiler_section(Vec::new());
    bytes.extend_from_slice(&overflowing_section_header());
    fs::write(&trace_path, bytes).expect("trace is written");

    let result = kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path);
    let Err(error) = result else {
        panic!("overflowing section length is rejected");
    };

    assert!(
        error
            .to_string()
            .contains("invalid profiler section length"),
        "{error:#}"
    );
}

#[tokio::test]
async fn build_skips_unsupported_profiler_sections() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("unsupported-section.hitrace");
    let mut bytes = profiler_section_body(99, vec![1, 2, 3]);
    bytes.extend_from_slice(&profiler_section(vec![TestProfilerPluginData {
        name: "ftrace-plugin".to_string(),
        status: 1,
        data: empty_trace_plugin_result(),
        clock_id: 2,
        tv_sec: 10,
        tv_nsec: 200,
        version: "1.0".to_string(),
        sample_interval: 16,
    }]));
    fs::write(&trace_path, bytes).expect("trace is written");

    let datasource =
        kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path).expect("datasource builds");
    let rows = datasource
        .query_json("select count(*) as count from profiler_plugin_data")
        .await
        .expect("query succeeds");

    assert_eq!(rows, json!([{ "count": 1 }]));
}

#[tokio::test]
async fn query_extracts_sched_switch_from_ftrace_plugin_result() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("sched-switch.hitrace");
    fs::write(
        &trace_path,
        profiler_section(vec![ftrace_plugin_with_sched_switch()]),
    )
    .expect("trace is written");

    let datasource =
        kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path).expect("datasource builds");
    let rows = datasource
        .query_json("select prev_comm, prev_pid, next_comm, next_pid from sched_switch limit 10")
        .await
        .expect("query succeeds");

    assert_eq!(
        rows,
        json!([{
            "prev_comm": "RenderThread",
            "prev_pid": 42,
            "next_comm": "com.tencent.mm",
            "next_pid": 100,
        }])
    );
}

#[tokio::test]
async fn query_json_converts_scalar_result_types() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("empty.hitrace");
    fs::write(&trace_path, profiler_section(Vec::new())).expect("trace is written");

    let datasource =
        kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path).expect("datasource builds");
    let rows = datasource
        .query_json(
            "select true as flag, \
             cast(1.5 as double) as double_value, \
             cast(2.5 as float) as float_value, \
             cast(null as int) as missing",
        )
        .await
        .expect("query succeeds");

    assert_eq!(
        rows,
        json!([{
            "flag": true,
            "double_value": 1.5,
            "float_value": 2.5,
            "missing": null,
        }])
    );
}

fn encoded_trace() -> Vec<u8> {
    let mut bytes = profiler_section(vec![TestProfilerPluginData {
        name: "ftrace-plugin_config".to_string(),
        status: 0,
        data: vec![1, 2, 3],
        clock_id: 2,
        tv_sec: 10,
        tv_nsec: 100,
        version: "1.0".to_string(),
        sample_interval: 8,
    }]);
    bytes.extend_from_slice(&profiler_section(vec![TestProfilerPluginData {
        name: "ftrace-plugin".to_string(),
        status: 1,
        data: empty_trace_plugin_result(),
        clock_id: 2,
        tv_sec: 10,
        tv_nsec: 200,
        version: "1.0".to_string(),
        sample_interval: 16,
    }]));
    bytes
}

fn overflowing_section_header() -> Vec<u8> {
    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    bytes[56..60].copy_from_slice(&HIPROFILER_PROTOBUF_BIN.to_le_bytes());
    bytes
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

fn ftrace_plugin_with_sched_switch() -> TestProfilerPluginData {
    let result = TestTracePluginResult {
        ftrace_cpu_detail: vec![TestFtraceCpuDetailMsg {
            cpu: 0,
            event: vec![TestFtraceEvent {
                sched_switch_format: Some(TestSchedSwitchFormat {
                    prev_comm: "RenderThread".to_string(),
                    prev_pid: 42,
                    prev_prio: 120,
                    prev_state: 1,
                    next_comm: "com.tencent.mm".to_string(),
                    next_pid: 100,
                    next_prio: 120,
                }),
            }],
            overwrite: 0,
        }],
    };

    TestProfilerPluginData {
        name: "ftrace-plugin".to_string(),
        status: 1,
        data: result.encode_to_vec(),
        clock_id: 2,
        tv_sec: 10,
        tv_nsec: 200,
        version: "1.0".to_string(),
        sample_interval: 16,
    }
}

fn empty_trace_plugin_result() -> Vec<u8> {
    TestTracePluginResult {
        ftrace_cpu_detail: Vec::new(),
    }
    .encode_to_vec()
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
