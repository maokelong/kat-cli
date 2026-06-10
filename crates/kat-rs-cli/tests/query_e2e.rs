use std::{fs, process::Command};

use prost::Message;
use serde_json::json;
use tempfile::tempdir;

const PROFILER_HEADER_SIZE: usize = 1024;
const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
const HIPROFILER_PROTOBUF_BIN: u32 = 0;

#[test]
fn query_prints_sched_switch_fields() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("sched-switch.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");

    let output = Command::new(env!("CARGO_BIN_EXE_kat-rs"))
        .args([
            "query",
            "--source",
            "hitrace",
            "--file",
            trace_path.to_str().expect("trace path is utf8"),
            "--sql",
            "select prev_comm, prev_pid, next_comm, next_pid from sched_switch",
        ])
        .output()
        .expect("kat-rs runs");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("stdout json"),
        json!([{
            "prev_comm": "render",
            "prev_pid": 42,
            "next_comm": "main",
            "next_pid": 7,
        }])
    );
}

#[test]
fn query_reports_malformed_hitrace_without_panic() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("malformed.hitrace");
    fs::write(&trace_path, overflowing_section_trace()).expect("trace is written");

    let output = Command::new(env!("CARGO_BIN_EXE_kat-rs"))
        .args([
            "query",
            "--source",
            "hitrace",
            "--file",
            trace_path.to_str().expect("trace path is utf8"),
            "--sql",
            "select 1",
        ])
        .output()
        .expect("kat-rs runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("invalid profiler section length"),
        "{stderr}"
    );
    assert!(!stderr.contains("panicked"), "{stderr}");
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
    #[prost(oneof = "test_ftrace_event::Event", tags = "2417")]
    event: Option<test_ftrace_event::Event>,
}

mod test_ftrace_event {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Event {
        #[prost(message, tag = "2417")]
        SchedSwitchFormat(super::TestSchedSwitchFormat),
    }
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
            cpu: 0,
            event: vec![TestFtraceEvent {
                event: Some(test_ftrace_event::Event::SchedSwitchFormat(
                    TestSchedSwitchFormat {
                        prev_comm: "render".to_string(),
                        prev_pid: 42,
                        prev_prio: 120,
                        prev_state: 1,
                        next_comm: "main".to_string(),
                        next_pid: 7,
                        next_prio: 100,
                    },
                )),
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

fn overflowing_section_trace() -> Vec<u8> {
    let mut bytes = profiler_section(Vec::new());
    bytes.extend_from_slice(&overflowing_section_header());
    bytes
}

fn profiler_section(plugins: Vec<TestProfilerPluginData>) -> Vec<u8> {
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

fn overflowing_section_header() -> Vec<u8> {
    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    bytes[56..60].copy_from_slice(&HIPROFILER_PROTOBUF_BIN.to_le_bytes());
    bytes
}

fn append_segment(bytes: &mut Vec<u8>, plugin: TestProfilerPluginData) {
    let segment = plugin.encode_to_vec();
    bytes.extend_from_slice(&(segment.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&segment);
}
