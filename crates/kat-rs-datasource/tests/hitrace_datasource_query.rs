// datasource 查询测试用最小 htrace 样本覆盖容器解析、插件流转和 SQL 查询行为。
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
async fn build_exposes_empty_profiler_table_without_profiler_records() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("unsupported-only-section.hitrace");
    fs::write(&trace_path, profiler_section_body(99, vec![1, 2, 3])).expect("trace is written");

    let datasource =
        kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path).expect("datasource builds");
    let rows = datasource
        .query_json("select count(*) as count from profiler_plugin_data")
        .await
        .expect("empty profiler_plugin_data table can be queried");

    assert_eq!(rows, json!([{ "count": 0 }]));
}

#[tokio::test]
async fn plugin_flow_dispatch_ignores_config_and_unknown_plugin_payloads() {
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

    let raw_rows = datasource
        .query_json("select name, data from profiler_plugin_data order by name")
        .await
        .expect("raw profiler table is queryable");
    assert_eq!(
        raw_rows,
        json!([
            { "name": "ftrace-plugin_config", "data": "010203" },
            { "name": "unknown-plugin", "data": "090909" }
        ])
    );

    let direct_rows = datasource
        .query_json("select count(*) as count from sched_switch")
        .await
        .expect("direct table remains queryable");
    assert_eq!(direct_rows, json!([{ "count": 0 }]));
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
            "select event_timestamp, event_cpu, event_comm, pid, caller, io_wait \
             from sched_blocked_reason",
        )
        .await
        .expect("sched_blocked_reason query succeeds");
    assert_eq!(
        rows,
        json!([{
            "event_timestamp": 20,
            "event_cpu": 3,
            "event_comm": "blocked_source",
            "pid": 42,
            "caller": 3735928559u64,
            "io_wait": 1,
        }])
    );

    let rows = datasource
        .query_json("select event_timestamp, event_cpu, comm, pid from sched_kthread_stop")
        .await
        .expect("sched_kthread_stop query succeeds");
    assert_eq!(
        rows,
        json!([{
            "event_timestamp": 25,
            "event_cpu": 3,
            "comm": "worker",
            "pid": 77,
        }])
    );

    let rows = datasource
        .query_json(
            "select event_timestamp, event_cpu, comm, pid, prio, orig_cpu, dest_cpu \
             from sched_migrate_task",
        )
        .await
        .expect("sched_migrate_task query succeeds");
    assert_eq!(
        rows,
        json!([{
            "event_timestamp": 30,
            "event_cpu": 3,
            "comm": "RenderThread",
            "pid": 42,
            "prio": 120,
            "orig_cpu": 1,
            "dest_cpu": 3,
        }])
    );

    let rows = datasource
        .query_json("select count(*) as count from sched_process_exec")
        .await
        .expect("empty sched_process_exec query succeeds");
    assert_eq!(rows, json!([{ "count": 0 }]));
}

#[tokio::test]
async fn query_json_converts_scalar_result_types() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("empty.hitrace");
    fs::write(&trace_path, profiler_section(Vec::new())).expect("trace is written");

    let datasource =
        kat_rs_datasource::TraceDatasource::from_hitrace(&trace_path).expect("datasource builds");
    let empty_table_rows = datasource
        .query_json("select count(*) as count from profiler_plugin_data")
        .await
        .expect("empty profiler_plugin_data table can be queried");
    assert_eq!(empty_table_rows, json!([{ "count": 0 }]));

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
