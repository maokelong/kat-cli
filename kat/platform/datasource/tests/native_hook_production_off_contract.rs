use std::fs;

use prost::Message;
use tempfile::tempdir;

#[allow(dead_code)]
mod proto {
    pub mod kat {
        pub mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }

        pub mod native_hook {
            include!(concat!(env!("OUT_DIR"), "/kat.native_hook.rs"));
        }
    }
}

use proto::kat::{
    hitrace::{
        FtraceCpuDetailMsg, FtraceCpuStatsMsg, FtraceEvent, PerCpuStatsMsg, ProfilerPluginData,
        SchedSwitchFormat, TracePluginResult, ftrace_cpu_stats_msg, profiler_plugin_data,
    },
    native_hook::{
        AllocEvent, BatchNativeHookData, NativeHookConfig, NativeHookData, native_hook_data,
    },
};

const PROFILER_HEADER_SIZE: usize = 1024;
const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;

#[test]
fn hitrace_staging_keeps_native_hook_source_tables_dormant() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("native-hook-and-ftrace.htrace");
    let dataset = root.path().join("dataset");
    fs::write(&source, generated_trace()).expect("generated Hitrace is written");

    let staged = kat_datasource::stage_hitrace(&source, &dataset, |_| Ok(()))
        .expect("Hitrace staging succeeds");

    let table_names = staged.table_names();
    assert_eq!(
        table_names,
        ["clock_domain", "clock_snapshot", "sched_switch"],
        "#195 only prepares dormant Native Hook Source capture; production publication stays off"
    );
    for dormant_table in [
        "profiler_payload_occurrence",
        "batch_native_hook_data",
        "native_hook_config",
    ] {
        assert!(!table_names.iter().any(|name| name == dormant_table));
    }
}

fn generated_trace() -> Vec<u8> {
    profiler_section(vec![
        profiler_envelope("ftrace-plugin", ftrace_payload().encode_to_vec()),
        profiler_envelope(
            "nativehook_config",
            NativeHookConfig {
                pid: 42,
                process_name: "render".to_owned(),
                clock: "boot".to_owned(),
                ..Default::default()
            }
            .encode_to_vec(),
        ),
        profiler_envelope(
            "nativehook",
            BatchNativeHookData {
                events: vec![NativeHookData {
                    tv_sec: 7,
                    tv_nsec: 8,
                    event: Some(native_hook_data::Event::AllocEvent(AllocEvent {
                        pid: 42,
                        tid: 43,
                        addr: 0x1000,
                        size: 64,
                        frame_info: Vec::new(),
                        thread_name_id: 9,
                        stack_id: 10,
                    })),
                }],
            }
            .encode_to_vec(),
        ),
    ])
}

fn ftrace_payload() -> TracePluginResult {
    let cpu_stats = PerCpuStatsMsg {
        cpu: 0,
        ..Default::default()
    };
    TracePluginResult {
        ftrace_cpu_stats: vec![
            FtraceCpuStatsMsg {
                status: ftrace_cpu_stats_msg::Status::TraceStart as i32,
                per_cpu_stats: vec![cpu_stats],
                trace_clock: "boot".to_owned(),
            },
            FtraceCpuStatsMsg {
                status: ftrace_cpu_stats_msg::Status::TraceEnd as i32,
                per_cpu_stats: vec![cpu_stats],
                trace_clock: "boot".to_owned(),
            },
        ],
        ftrace_cpu_detail: vec![FtraceCpuDetailMsg {
            cpu: 0,
            event: vec![FtraceEvent {
                timestamp: 99,
                sched_switch_format: Some(SchedSwitchFormat {
                    prev_comm: "swapper".to_owned(),
                    prev_pid: 0,
                    prev_prio: 120,
                    prev_state: 0,
                    next_comm: "render".to_owned(),
                    next_pid: 42,
                    next_prio: 120,
                }),
                ..Default::default()
            }],
            overwrite: 0,
        }],
        symbols_detail: Vec::new(),
        clocks_detail: Vec::new(),
        version: "1.0".to_owned(),
    }
}

fn profiler_envelope(name: &str, data: Vec<u8>) -> ProfilerPluginData {
    ProfilerPluginData {
        name: name.to_owned(),
        status: u32::from(!name.ends_with("_config")),
        data,
        clock_id: profiler_plugin_data::ClockId::ClockidBoottime as i32,
        tv_sec: 10,
        tv_nsec: 20,
        version: "1.0".to_owned(),
        sample_interval: 10,
    }
}

fn profiler_section(envelopes: Vec<ProfilerPluginData>) -> Vec<u8> {
    let mut body = Vec::new();
    for envelope in envelopes {
        let frame = envelope.encode_to_vec();
        body.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        body.extend_from_slice(&frame);
    }

    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((PROFILER_HEADER_SIZE + body.len()) as u64).to_le_bytes());
    for (offset, value) in [60, 68, 76, 84, 92, 100].into_iter().zip(101_u64..=106) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&body);
    bytes
}
