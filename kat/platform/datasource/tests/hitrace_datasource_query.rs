use std::fs;

use kat_datasource as kat_rs_datasource;
use prost::{Message, Oneof};
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
            "select prev_comm, prev_pid, next_comm, next_pid \
             from trace_plugin_result__ftrace_cpu_detail__event__sched_switch_format",
        )
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

#[test]
fn build_rejects_profiler_envelope_frames_without_hitrace_header() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("frame-only.hitrace");
    let mut bytes = Vec::new();
    append_profiler_envelope_frame(
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
        panic!("frame-only input is rejected");
    };

    assert!(
        format!("{error:#}").contains("missing OHOSPROF header"),
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
        format!("{error:#}").contains("invalid profiler section length"),
        "{error:#}"
    );
}

#[tokio::test]
async fn build_skips_unsupported_profiler_sections() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("unsupported-section.hitrace");
    let mut bytes = profiler_section_body(99, vec![1, 2, 3]);
    bytes.extend_from_slice(&profiler_section(vec![ftrace_plugin_with_sched_switch()]));
    fs::write(&trace_path, bytes).expect("trace is written");

    let datasource =
        kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path).expect("datasource builds");
    let rows = datasource
        .query_json("select count(*) as count from trace_plugin_result__ftrace_cpu_detail__event__sched_switch_format")
        .await
        .expect("query succeeds");

    assert_eq!(rows, json!([{ "count": 1 }]));
}

#[tokio::test]
async fn build_exposes_empty_profiler_table_without_profiler_records() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("unsupported-only-section.hitrace");
    fs::write(&trace_path, profiler_section_body(99, vec![1, 2, 3])).expect("trace is written");

    let datasource =
        kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path).expect("datasource builds");
    assert!(
        datasource
            .query_json("select count(*) as count from profiler_plugin_data")
            .await
            .is_err(),
        "old raw profiler table should not be registered"
    );
}

#[tokio::test]
async fn profiler_dispatch_ignores_config_and_unknown_plugin_payloads() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("plugin-flow-envelopes.hitrace");
    fs::write(
        &trace_path,
        profiler_section(vec![
            TestProfilerPluginData {
                name: "ftrace-plugin_config".to_string(),
                status: 0,
                data: vec![1, 2, 3],
                clock_id: 2,
                tv_sec: 10,
                tv_nsec: 100,
                version: "1.0".to_string(),
                sample_interval: 8,
            },
            TestProfilerPluginData {
                name: "unknown-plugin".to_string(),
                status: 0,
                data: vec![9, 9, 9],
                clock_id: 2,
                tv_sec: 11,
                tv_nsec: 200,
                version: "1.0".to_string(),
                sample_interval: 9,
            },
        ]),
    )
    .expect("trace is written");

    let datasource =
        kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path).expect("datasource builds");

    assert!(
        datasource
            .query_json("select name, data from profiler_plugin_data order by name")
            .await
            .is_err(),
        "old raw profiler table should not be registered"
    );
    assert!(
        datasource
            .query_json("select count(*) as count from sched_switch")
            .await
            .is_err(),
        "old direct sched_switch table should not be registered"
    );
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
        .query_json(
            "select prev_comm, prev_pid, next_comm, next_pid \
             from trace_plugin_result__ftrace_cpu_detail__event__sched_switch_format limit 10",
        )
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
async fn query_extracts_direct_sched_event_tables() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("sched-events.hitrace");
    fs::write(
        &trace_path,
        profiler_section(vec![ftrace_plugin_with_sched_events()]),
    )
    .expect("trace is written");

    let datasource =
        kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path).expect("datasource builds");

    let rows = datasource
        .query_json(
            "select e.timestamp as event_timestamp, c.cpu as event_cpu, e.comm as event_comm, \
                    s.prev_comm, s.next_comm \
             from trace_plugin_result__ftrace_cpu_detail__event__sched_switch_format s \
             join trace_plugin_result__ftrace_cpu_detail__event e \
               on s.source_index = e.source_index \
              and s.parent_index = e.row_index \
             join trace_plugin_result__ftrace_cpu_detail c \
               on e.source_index = c.source_index \
              and e.parent_index = c.row_index",
        )
        .await
        .expect("sched_switch prototype table query succeeds");
    assert_eq!(
        rows,
        json!([{
            "event_timestamp": 10u64,
            "event_cpu": 3,
            "event_comm": "switch_source",
            "prev_comm": "RenderThread",
            "next_comm": "main",
        }])
    );
    assert!(
        datasource
            .query_json("select count(*) as count from sched_blocked_reason")
            .await
            .is_err(),
        "old non-prototype ftrace table should not be registered"
    );
}

#[tokio::test]
async fn query_extracts_native_hook_config_and_direct_tables() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("native-hook.hitrace");
    fs::write(
        &trace_path,
        profiler_section(vec![
            native_hook_config_plugin(),
            native_hook_plugin_with_events(),
        ]),
    )
    .expect("trace is written");

    let datasource =
        kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path).expect("datasource builds");

    let alloc_rows = datasource
        .query_json(
            "select pid, tid, addr, size, thread_name_id, stack_id \
             from batch_native_hook_data__events__alloc_event",
        )
        .await
        .expect("native_hook_alloc query succeeds");
    assert_eq!(
        alloc_rows,
        json!([{
            "pid": 42,
            "tid": 43,
            "addr": 4096u64,
            "size": 64u64,
            "thread_name_id": 7,
            "stack_id": 8,
        }])
    );

    let expanded_pids = datasource
        .query_json("select value from native_hook_config__expand_pids order by row_index")
        .await
        .expect("repeated int config field is expanded");
    assert_eq!(expanded_pids, json!([{ "value": 42 }, { "value": 77 }]));

    let restrace_tags = datasource
        .query_json("select value from native_hook_config__restrace_tag order by row_index")
        .await
        .expect("repeated string config field is expanded");
    assert_eq!(restrace_tags, json!([{ "value": "fd" }, { "value": "vm" }]));

    let event_rows = datasource
        .query_json("select event from batch_native_hook_data__events order by row_index")
        .await
        .expect("oneof parent table records selected variant name");
    assert_eq!(
        event_rows,
        json!([
            { "event": "alloc_event" },
            { "event": "statistics_event" },
            { "event": "trace_alloc_event" },
            { "event": "trace_free_event" },
            { "event": "maps_info" },
            { "event": "symbol_tab" },
            { "event": "stack_map" },
        ])
    );

    let trace_alloc_rows = datasource
        .query_json(
            "select trace_type, trace_type_name \
             from batch_native_hook_data__events__trace_alloc_event",
        )
        .await
        .expect("enum field exposes raw value and name");
    assert_eq!(
        trace_alloc_rows,
        json!([{ "trace_type": 0, "trace_type_name": "FD" }])
    );

    let statistics_rows = datasource
        .query_json(
            "select type as memory_type, type_name \
             from batch_native_hook_data__events__statistics_event",
        )
        .await
        .expect("nested enum field exposes raw value and name");
    assert_eq!(
        statistics_rows,
        json!([{ "memory_type": 1, "type_name": "MMAP" }])
    );

    let symbol_rows = datasource
        .query_json(
            "select sym_table, str_table \
             from batch_native_hook_data__events__symbol_tab",
        )
        .await
        .expect("bytes fields are exposed as binary columns");
    assert_eq!(
        symbol_rows,
        json!([{ "sym_table": "010203", "str_table": "0405" }])
    );

    let frame_map_ids = datasource
        .query_json(
            "select value \
             from batch_native_hook_data__events__stack_map__frame_map_id \
             order by row_index",
        )
        .await
        .expect("nested repeated uint64 field is expanded");
    assert_eq!(
        frame_map_ids,
        json!([{ "value": 10u64 }, { "value": 11u64 }])
    );

    assert!(
        datasource
            .query_json("select event_ts from batch_native_hook_data__events__alloc_event")
            .await
            .is_err(),
        "native hook alloc should not expose derived event_ts"
    );
    assert!(
        datasource
            .query_json("select count(*) from native_hook_alloc")
            .await
            .is_err(),
        "old native_hook_alloc table should not be registered"
    );
}

#[tokio::test]
async fn query_extracts_fixed_result_system_plugin_direct_tables() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("fixed-result.hitrace");
    fs::write(&trace_path, profiler_section(fixed_result_system_plugins()))
        .expect("trace is written");

    let datasource =
        kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path).expect("datasource builds");

    let rows = datasource
        .query_json("select zram, gpu_used_size from memory_data")
        .await
        .expect("memory_data query succeeds");
    assert_eq!(rows, json!([{ "zram": 64u64, "gpu_used_size": 32u64 }]));

    assert!(
        datasource
            .query_json("select count(*) from process_data_processesinfo")
            .await
            .is_err(),
        "old fixed_result child table should not be registered"
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
    bytes.extend_from_slice(&profiler_section(vec![ftrace_plugin_with_sched_switch()]));
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
struct TestNativeHookConfig {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(string, tag = "7")]
    process_name: String,
    #[prost(uint32, tag = "22")]
    statistics_interval: u32,
    #[prost(uint32, tag = "24")]
    sample_interval: u32,
    #[prost(int32, repeated, tag = "26")]
    expand_pids: Vec<i32>,
    #[prost(string, tag = "29")]
    filter_napi_name: String,
    #[prost(bool, tag = "30")]
    dump_nmd: bool,
    #[prost(string, tag = "31")]
    target_so_name: String,
    #[prost(string, repeated, tag = "32")]
    restrace_tag: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
struct TestBatchNativeHookData {
    #[prost(message, repeated, tag = "1")]
    events: Vec<TestNativeHookData>,
}

#[derive(Clone, PartialEq, Message)]
struct TestNativeHookData {
    #[prost(uint64, tag = "1")]
    tv_sec: u64,
    #[prost(uint64, tag = "2")]
    tv_nsec: u64,
    #[prost(oneof = "TestNativeHookEvent", tags = "3, 11, 15, 16, 17")]
    event: Option<TestNativeHookEvent>,
}

#[derive(Clone, PartialEq, Oneof)]
#[allow(clippy::enum_variant_names)]
enum TestNativeHookEvent {
    #[prost(message, tag = "3")]
    AllocEvent(TestAllocEvent),
    #[prost(message, tag = "11")]
    MapsInfo(TestMapsInfo),
    #[prost(message, tag = "12")]
    SymbolTab(TestSymbolTable),
    #[prost(message, tag = "14")]
    StackMap(TestStackMap),
    #[prost(message, tag = "15")]
    StatisticsEvent(TestRecordStatisticsEvent),
    #[prost(message, tag = "16")]
    TraceAllocEvent(TestTraceAllocEvent),
    #[prost(message, tag = "17")]
    TraceFreeEvent(TestTraceFreeEvent),
}

#[derive(Clone, PartialEq, Message)]
struct TestMapsInfo {
    #[prost(uint32, tag = "1")]
    pid: u32,
    #[prost(uint64, tag = "2")]
    start: u64,
    #[prost(uint64, tag = "3")]
    end: u64,
    #[prost(uint64, tag = "4")]
    offset: u64,
    #[prost(uint32, tag = "5")]
    file_path_id: u32,
}

#[derive(Clone, PartialEq, Message)]
struct TestSymbolTable {
    #[prost(uint32, tag = "1")]
    file_path_id: u32,
    #[prost(uint64, tag = "2")]
    text_exec_vaddr: u64,
    #[prost(uint64, tag = "3")]
    text_exec_vaddr_file_offset: u64,
    #[prost(uint32, tag = "4")]
    sym_entry_size: u32,
    #[prost(bytes = "vec", tag = "5")]
    sym_table: Vec<u8>,
    #[prost(bytes = "vec", tag = "6")]
    str_table: Vec<u8>,
    #[prost(int32, tag = "7")]
    pid: i32,
}

#[derive(Clone, PartialEq, Message)]
struct TestStackMap {
    #[prost(uint32, tag = "1")]
    id: u32,
    #[prost(uint64, repeated, tag = "2")]
    frame_map_id: Vec<u64>,
    #[prost(uint64, repeated, tag = "3")]
    ip: Vec<u64>,
    #[prost(int32, tag = "4")]
    pid: i32,
}

#[derive(Clone, PartialEq, Message)]
struct TestAllocEvent {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(int32, tag = "2")]
    tid: i32,
    #[prost(uint64, tag = "3")]
    addr: u64,
    #[prost(uint64, tag = "4")]
    size: u64,
    #[prost(uint32, tag = "6")]
    thread_name_id: u32,
    #[prost(uint32, tag = "7")]
    stack_id: u32,
}

#[derive(Clone, PartialEq, Message)]
struct TestRecordStatisticsEvent {
    #[prost(uint32, tag = "1")]
    pid: u32,
    #[prost(uint32, tag = "2")]
    callstack_id: u32,
    #[prost(int32, tag = "3")]
    r#type: i32,
    #[prost(uint64, tag = "4")]
    apply_count: u64,
    #[prost(uint64, tag = "5")]
    release_count: u64,
    #[prost(uint64, tag = "6")]
    apply_size: u64,
    #[prost(uint64, tag = "7")]
    release_size: u64,
    #[prost(string, tag = "8")]
    tag_name: String,
}

#[derive(Clone, PartialEq, Message)]
struct TestTraceAllocEvent {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(int32, tag = "2")]
    tid: i32,
    #[prost(uint64, tag = "3")]
    addr: u64,
    #[prost(int32, tag = "4")]
    trace_type: i32,
    #[prost(string, tag = "5")]
    tag_name: String,
    #[prost(uint64, tag = "6")]
    size: u64,
    #[prost(uint32, tag = "8")]
    thread_name_id: u32,
    #[prost(uint32, tag = "9")]
    stack_id: u32,
}

#[derive(Clone, PartialEq, Message)]
struct TestTraceFreeEvent {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(int32, tag = "2")]
    tid: i32,
    #[prost(uint64, tag = "3")]
    addr: u64,
    #[prost(int32, tag = "4")]
    trace_type: i32,
    #[prost(string, tag = "5")]
    tag_name: String,
    #[prost(uint32, tag = "7")]
    thread_name_id: u32,
    #[prost(uint32, tag = "8")]
    stack_id: u32,
}

#[derive(Clone, PartialEq, Message)]
struct TestCpuConfig {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(bool, tag = "2")]
    report_process_info: bool,
}

#[derive(Clone, PartialEq, Message)]
struct TestCpuData {
    #[prost(int64, tag = "3")]
    process_num: i64,
    #[prost(double, tag = "4")]
    user_load: f64,
    #[prost(double, tag = "5")]
    sys_load: f64,
    #[prost(double, tag = "6")]
    total_load: f64,
}

#[derive(Clone, PartialEq, Message)]
struct TestMemoryConfig {
    #[prost(bool, tag = "2")]
    report_sysmem_mem_info: bool,
}

#[derive(Clone, PartialEq, Message)]
struct TestMemoryData {
    #[prost(uint64, tag = "4")]
    zram: u64,
    #[prost(uint64, tag = "10")]
    gpu_used_size: u64,
}

#[derive(Clone, PartialEq, Message)]
struct TestProcessConfig {
    #[prost(bool, tag = "1")]
    report_process_tree: bool,
    #[prost(bool, tag = "2")]
    report_cpu: bool,
}

#[derive(Clone, PartialEq, Message)]
struct TestProcessData {
    #[prost(message, repeated, tag = "1")]
    processesinfo: Vec<TestProcessInfo>,
}

#[derive(Clone, PartialEq, Message)]
struct TestProcessInfo {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(string, tag = "2")]
    name: String,
}

#[derive(Clone, PartialEq, Message)]
struct TestDiskioConfig {
    #[prost(int32, tag = "2")]
    report_io_stats: i32,
}

#[derive(Clone, PartialEq, Message)]
struct TestDiskioData {
    #[prost(int64, tag = "4")]
    rd_sectors_kb: i64,
    #[prost(int64, tag = "5")]
    wr_sectors_kb: i64,
}

#[derive(Clone, PartialEq, Message)]
struct TestNetworkConfig {
    #[prost(int32, tag = "3")]
    single_pid: i32,
    #[prost(string, tag = "4")]
    startup_process_name: String,
}

#[derive(Clone, PartialEq, Message)]
struct TestNetworkDatas {
    #[prost(message, repeated, tag = "1")]
    networkinfo: Vec<TestNetworkData>,
}

#[derive(Clone, PartialEq, Message)]
struct TestNetworkData {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(uint64, tag = "4")]
    tx_bytes: u64,
    #[prost(uint64, tag = "5")]
    rx_bytes: u64,
}

#[derive(Clone, PartialEq, Message)]
struct TestGpuConfig {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(bool, tag = "2")]
    report_gpu_info: bool,
}

#[derive(Clone, PartialEq, Message)]
struct TestGpuData {
    #[prost(uint64, tag = "1")]
    boottime: u64,
    #[prost(uint64, tag = "2")]
    gpu_utilisation: u64,
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
    #[prost(message, optional, tag = "2400")]
    sched_kthread_stop_format: Option<TestSchedKthreadStopFormat>,
    #[prost(message, optional, tag = "2402")]
    sched_migrate_task_format: Option<TestSchedMigrateTaskFormat>,
    #[prost(message, optional, tag = "2417")]
    sched_switch_format: Option<TestSchedSwitchFormat>,
    #[prost(message, optional, tag = "2420")]
    sched_wakeup_format: Option<TestSchedWakeupFormat>,
    #[prost(message, optional, tag = "2421")]
    sched_wakeup_new_format: Option<TestSchedWakeupFormat>,
    #[prost(message, optional, tag = "2422")]
    sched_waking_format: Option<TestSchedWakeupFormat>,
    #[prost(message, optional, tag = "4002")]
    sched_blocked_reason_format: Option<TestSchedBlockedReasonFormat>,
}

#[derive(Clone, PartialEq, Message)]
struct TestSchedBlockedReasonFormat {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(uint64, tag = "2")]
    caller: u64,
    #[prost(uint32, tag = "3")]
    io_wait: u32,
    #[prost(string, tag = "4")]
    caller_str: String,
}

#[derive(Clone, PartialEq, Message)]
struct TestSchedKthreadStopFormat {
    #[prost(string, tag = "1")]
    comm: String,
    #[prost(int32, tag = "2")]
    pid: i32,
}

#[derive(Clone, PartialEq, Message)]
struct TestSchedMigrateTaskFormat {
    #[prost(string, tag = "1")]
    comm: String,
    #[prost(int32, tag = "2")]
    pid: i32,
    #[prost(int32, tag = "3")]
    prio: i32,
    #[prost(int32, tag = "4")]
    orig_cpu: i32,
    #[prost(int32, tag = "5")]
    dest_cpu: i32,
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
struct TestSchedWakeupFormat {
    #[prost(string, tag = "1")]
    comm: String,
    #[prost(int32, tag = "2")]
    pid: i32,
    #[prost(int32, tag = "3")]
    prio: i32,
    #[prost(int32, tag = "4")]
    success: i32,
    #[prost(int32, tag = "5")]
    target_cpu: i32,
}

fn ftrace_plugin_with_sched_switch() -> TestProfilerPluginData {
    let result = TestTracePluginResult {
        ftrace_cpu_detail: vec![TestFtraceCpuDetailMsg {
            cpu: 0,
            event: vec![TestFtraceEvent {
                timestamp: 10,
                tgid: 500,
                comm: "switch_source".to_string(),
                sched_kthread_stop_format: None,
                sched_migrate_task_format: None,
                sched_switch_format: Some(TestSchedSwitchFormat {
                    prev_comm: "RenderThread".to_string(),
                    prev_pid: 42,
                    prev_prio: 120,
                    prev_state: 1,
                    next_comm: "com.tencent.mm".to_string(),
                    next_pid: 100,
                    next_prio: 120,
                }),
                sched_wakeup_format: None,
                sched_wakeup_new_format: None,
                sched_waking_format: None,
                sched_blocked_reason_format: None,
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

fn ftrace_plugin_with_sched_events() -> TestProfilerPluginData {
    let result = TestTracePluginResult {
        ftrace_cpu_detail: vec![TestFtraceCpuDetailMsg {
            cpu: 3,
            event: vec![
                TestFtraceEvent {
                    timestamp: 10,
                    tgid: 500,
                    comm: "switch_source".to_string(),
                    sched_kthread_stop_format: None,
                    sched_migrate_task_format: None,
                    sched_switch_format: Some(TestSchedSwitchFormat {
                        prev_comm: "RenderThread".to_string(),
                        prev_pid: 42,
                        prev_prio: 120,
                        prev_state: 1,
                        next_comm: "main".to_string(),
                        next_pid: 100,
                        next_prio: 120,
                    }),
                    sched_wakeup_format: None,
                    sched_wakeup_new_format: None,
                    sched_waking_format: None,
                    sched_blocked_reason_format: None,
                },
                TestFtraceEvent {
                    timestamp: 20,
                    tgid: 500,
                    comm: "blocked_source".to_string(),
                    sched_kthread_stop_format: None,
                    sched_migrate_task_format: None,
                    sched_switch_format: None,
                    sched_wakeup_format: None,
                    sched_wakeup_new_format: None,
                    sched_waking_format: None,
                    sched_blocked_reason_format: Some(TestSchedBlockedReasonFormat {
                        pid: 42,
                        caller: 0xdead_beef,
                        io_wait: 1,
                        caller_str: "finish_task_switch".to_string(),
                    }),
                },
                TestFtraceEvent {
                    timestamp: 25,
                    tgid: 500,
                    comm: "kthread_source".to_string(),
                    sched_kthread_stop_format: Some(TestSchedKthreadStopFormat {
                        comm: "worker".to_string(),
                        pid: 77,
                    }),
                    sched_migrate_task_format: None,
                    sched_switch_format: None,
                    sched_wakeup_format: None,
                    sched_wakeup_new_format: None,
                    sched_waking_format: None,
                    sched_blocked_reason_format: None,
                },
                TestFtraceEvent {
                    timestamp: 30,
                    tgid: 500,
                    comm: "migrate_source".to_string(),
                    sched_kthread_stop_format: None,
                    sched_migrate_task_format: Some(TestSchedMigrateTaskFormat {
                        comm: "RenderThread".to_string(),
                        pid: 42,
                        prio: 120,
                        orig_cpu: 1,
                        dest_cpu: 3,
                    }),
                    sched_switch_format: None,
                    sched_wakeup_format: None,
                    sched_wakeup_new_format: None,
                    sched_waking_format: None,
                    sched_blocked_reason_format: None,
                },
                TestFtraceEvent {
                    timestamp: 40,
                    tgid: 500,
                    comm: "wakeup_source".to_string(),
                    sched_kthread_stop_format: None,
                    sched_migrate_task_format: None,
                    sched_switch_format: None,
                    sched_wakeup_format: Some(TestSchedWakeupFormat {
                        comm: "main".to_string(),
                        pid: 100,
                        prio: 120,
                        success: 1,
                        target_cpu: 3,
                    }),
                    sched_wakeup_new_format: None,
                    sched_waking_format: None,
                    sched_blocked_reason_format: None,
                },
                TestFtraceEvent {
                    timestamp: 50,
                    tgid: 500,
                    comm: "wakeup_new_source".to_string(),
                    sched_kthread_stop_format: None,
                    sched_migrate_task_format: None,
                    sched_switch_format: None,
                    sched_wakeup_format: None,
                    sched_wakeup_new_format: Some(TestSchedWakeupFormat {
                        comm: "new".to_string(),
                        pid: 101,
                        prio: 121,
                        success: 1,
                        target_cpu: 2,
                    }),
                    sched_waking_format: None,
                    sched_blocked_reason_format: None,
                },
                TestFtraceEvent {
                    timestamp: 60,
                    tgid: 500,
                    comm: "waking_source".to_string(),
                    sched_kthread_stop_format: None,
                    sched_migrate_task_format: None,
                    sched_switch_format: None,
                    sched_wakeup_format: None,
                    sched_wakeup_new_format: None,
                    sched_waking_format: Some(TestSchedWakeupFormat {
                        comm: "waking".to_string(),
                        pid: 102,
                        prio: 122,
                        success: 1,
                        target_cpu: 1,
                    }),
                    sched_blocked_reason_format: None,
                },
            ],
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

fn native_hook_config_plugin() -> TestProfilerPluginData {
    TestProfilerPluginData {
        name: "nativehook_config".to_string(),
        status: 0,
        data: TestNativeHookConfig {
            pid: 42,
            process_name: "render".to_string(),
            statistics_interval: 5,
            sample_interval: 10,
            expand_pids: vec![42, 77],
            filter_napi_name: "napi".to_string(),
            dump_nmd: true,
            target_so_name: "libark_jsruntime.so".to_string(),
            restrace_tag: vec!["fd".to_string(), "vm".to_string()],
        }
        .encode_to_vec(),
        clock_id: 2,
        tv_sec: 10,
        tv_nsec: 100,
        version: "1.0".to_string(),
        sample_interval: 10,
    }
}

fn native_hook_plugin_with_events() -> TestProfilerPluginData {
    TestProfilerPluginData {
        name: "nativehook".to_string(),
        status: 1,
        data: TestBatchNativeHookData {
            events: vec![
                TestNativeHookData {
                    tv_sec: 1,
                    tv_nsec: 20,
                    event: Some(TestNativeHookEvent::AllocEvent(TestAllocEvent {
                        pid: 42,
                        tid: 43,
                        addr: 0x1000,
                        size: 64,
                        thread_name_id: 7,
                        stack_id: 8,
                    })),
                },
                TestNativeHookData {
                    tv_sec: 2,
                    tv_nsec: 30,
                    event: Some(TestNativeHookEvent::StatisticsEvent(
                        TestRecordStatisticsEvent {
                            pid: 42,
                            callstack_id: 9,
                            r#type: 1,
                            apply_count: 3,
                            release_count: 1,
                            apply_size: 256,
                            release_size: 128,
                            tag_name: "ashmem".to_string(),
                        },
                    )),
                },
                TestNativeHookData {
                    tv_sec: 3,
                    tv_nsec: 40,
                    event: Some(TestNativeHookEvent::TraceAllocEvent(TestTraceAllocEvent {
                        pid: 42,
                        tid: 44,
                        addr: 0x2000,
                        trace_type: 0,
                        tag_name: "fd".to_string(),
                        size: 16,
                        thread_name_id: 11,
                        stack_id: 12,
                    })),
                },
                TestNativeHookData {
                    tv_sec: 4,
                    tv_nsec: 50,
                    event: Some(TestNativeHookEvent::TraceFreeEvent(TestTraceFreeEvent {
                        pid: 42,
                        tid: 44,
                        addr: 0x2000,
                        trace_type: 0,
                        tag_name: "fd".to_string(),
                        thread_name_id: 11,
                        stack_id: 12,
                    })),
                },
                TestNativeHookData {
                    tv_sec: 5,
                    tv_nsec: 60,
                    event: Some(TestNativeHookEvent::MapsInfo(TestMapsInfo {
                        pid: 42,
                        start: 0x3000,
                        end: 0x4000,
                        offset: 0x80,
                        file_path_id: 9,
                    })),
                },
                TestNativeHookData {
                    tv_sec: 6,
                    tv_nsec: 70,
                    event: Some(TestNativeHookEvent::SymbolTab(TestSymbolTable {
                        file_path_id: 9,
                        text_exec_vaddr: 0x5000,
                        text_exec_vaddr_file_offset: 0x40,
                        sym_entry_size: 16,
                        sym_table: vec![1, 2, 3],
                        str_table: vec![4, 5],
                        pid: 42,
                    })),
                },
                TestNativeHookData {
                    tv_sec: 7,
                    tv_nsec: 80,
                    event: Some(TestNativeHookEvent::StackMap(TestStackMap {
                        id: 99,
                        frame_map_id: vec![10, 11],
                        ip: vec![0x100, 0x200],
                        pid: 42,
                    })),
                },
            ],
        }
        .encode_to_vec(),
        clock_id: 2,
        tv_sec: 10,
        tv_nsec: 200,
        version: "1.0".to_string(),
        sample_interval: 10,
    }
}

fn fixed_result_system_plugins() -> Vec<TestProfilerPluginData> {
    vec![
        fixed_result_plugin(
            "cpu-plugin_config",
            TestCpuConfig {
                pid: 42,
                report_process_info: true,
            },
        ),
        fixed_result_plugin(
            "cpu-plugin",
            TestCpuData {
                process_num: 2,
                user_load: 1.5,
                sys_load: 2.5,
                total_load: 4.0,
            },
        ),
        fixed_result_plugin(
            "memory-plugin_config",
            TestMemoryConfig {
                report_sysmem_mem_info: true,
            },
        ),
        fixed_result_plugin(
            "memory-plugin",
            TestMemoryData {
                zram: 64,
                gpu_used_size: 32,
            },
        ),
        fixed_result_plugin(
            "process-plugin_config",
            TestProcessConfig {
                report_process_tree: true,
                report_cpu: true,
            },
        ),
        fixed_result_plugin(
            "process-plugin",
            TestProcessData {
                processesinfo: vec![TestProcessInfo {
                    pid: 42,
                    name: "render".to_string(),
                }],
            },
        ),
        fixed_result_plugin(
            "diskio-plugin_config",
            TestDiskioConfig { report_io_stats: 2 },
        ),
        fixed_result_plugin(
            "diskio-plugin",
            TestDiskioData {
                rd_sectors_kb: 10,
                wr_sectors_kb: 20,
            },
        ),
        fixed_result_plugin(
            "network-plugin_config",
            TestNetworkConfig {
                single_pid: 42,
                startup_process_name: "render".to_string(),
            },
        ),
        fixed_result_plugin(
            "network-plugin",
            TestNetworkDatas {
                networkinfo: vec![TestNetworkData {
                    pid: 42,
                    tx_bytes: 100,
                    rx_bytes: 200,
                }],
            },
        ),
        fixed_result_plugin(
            "gpu-plugin_config",
            TestGpuConfig {
                pid: 42,
                report_gpu_info: true,
            },
        ),
        fixed_result_plugin(
            "gpu-plugin",
            TestGpuData {
                boottime: 100,
                gpu_utilisation: 80,
            },
        ),
    ]
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

fn profiler_section(plugins: Vec<TestProfilerPluginData>) -> Vec<u8> {
    let mut body = Vec::new();
    for plugin in plugins {
        append_profiler_envelope_frame(&mut body, plugin);
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

fn append_profiler_envelope_frame(bytes: &mut Vec<u8>, plugin: TestProfilerPluginData) {
    let frame = plugin.encode_to_vec();
    bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&frame);
}
