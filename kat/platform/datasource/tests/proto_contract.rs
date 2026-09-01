use std::fs;

use prost::Message;

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

#[test]
fn generated_proto_includes_sched_switch_format() {
    let value = proto::kat::hitrace::SchedSwitchFormat {
        prev_comm: "render".to_string(),
        prev_pid: 42,
        prev_prio: 120,
        prev_state: 1,
        next_comm: "main".to_string(),
        next_pid: 7,
        next_prio: 100,
    };

    let decoded = proto::kat::hitrace::SchedSwitchFormat::decode(value.encode_to_vec().as_slice())
        .expect("decode");

    assert_eq!(decoded.prev_comm, "render");
    assert_eq!(decoded.prev_pid, 42);
    assert_eq!(decoded.next_comm, "main");
    assert_eq!(decoded.next_pid, 7);
}

#[test]
fn generated_proto_includes_upstream_sched_messages() {
    let value = proto::kat::hitrace::SchedBlockedReasonFormat {
        pid: 42,
        caller: 0xfeed_beef,
        io_wait: 1,
        caller_str: "finish_task_switch".to_string(),
    };

    let decoded =
        proto::kat::hitrace::SchedBlockedReasonFormat::decode(value.encode_to_vec().as_slice())
            .expect("decode");

    assert_eq!(decoded.pid, 42);
    assert_eq!(decoded.caller, 0xfeed_beef);
    assert_eq!(decoded.io_wait, 1);
    assert_eq!(decoded.caller_str, "finish_task_switch");
}

#[test]
fn generated_ftrace_event_uses_canonical_event_oneof() {
    let value = proto::kat::hitrace::FtraceEvent {
        timestamp: 10,
        tgid: 500,
        comm: "source".to_string(),
        event: Some(proto::kat::hitrace::ftrace_event::Event::SchedSwitchFormat(
            proto::kat::hitrace::SchedSwitchFormat {
                prev_comm: "render".to_string(),
                prev_pid: 42,
                next_comm: "main".to_string(),
                next_pid: 7,
                ..Default::default()
            },
        )),
        common_fields: Some(proto::kat::hitrace::ftrace_event::CommonFileds {
            r#type: 123,
            flags: 1,
            preempt_count: 2,
            pid: 42,
        }),
    };

    let decoded =
        proto::kat::hitrace::FtraceEvent::decode(value.encode_to_vec().as_slice()).expect("decode");

    assert!(matches!(
        decoded.event,
        Some(proto::kat::hitrace::ftrace_event::Event::SchedSwitchFormat(
            _
        ))
    ));
    assert_eq!(decoded.common_fields.expect("common fields").pid, 42);
}

#[test]
fn generated_proto_includes_native_hook_config_and_events() {
    let config = proto::kat::native_hook::NativeHookConfig {
        pid: 42,
        save_file: true,
        file_name: "native-hook.bin".to_string(),
        clock: "boottime".to_string(),
        expand_pids: vec![42, 77],
        ..Default::default()
    };
    let decoded =
        proto::kat::native_hook::NativeHookConfig::decode(config.encode_to_vec().as_slice())
            .expect("decode");
    assert_eq!(decoded.pid, 42);
    assert_eq!(decoded.expand_pids, vec![42, 77]);

    let batch = proto::kat::native_hook::BatchNativeHookData {
        events: vec![proto::kat::native_hook::NativeHookData {
            tv_sec: 1,
            tv_nsec: 20,
            event: Some(
                proto::kat::native_hook::native_hook_data::Event::AllocEvent(
                    proto::kat::native_hook::AllocEvent {
                        pid: 42,
                        tid: 43,
                        addr: 0x1000,
                        size: 64,
                        ..Default::default()
                    },
                ),
            ),
        }],
    };
    let decoded =
        proto::kat::native_hook::BatchNativeHookData::decode(batch.encode_to_vec().as_slice())
            .expect("decode");
    assert!(matches!(
        decoded.events[0].event,
        Some(proto::kat::native_hook::native_hook_data::Event::AllocEvent(_))
    ));
}

#[test]
fn ftrace_payload_messages_live_under_ftrace_data_proto() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let profiler_plugin_data = fs::read_to_string(format!(
        "{manifest_dir}/proto/profiler/profiler_plugin_data.proto"
    ))
    .expect("profiler plugin data proto source can be read");
    let ftrace_event = fs::read_to_string(format!(
        "{manifest_dir}/proto/ftrace_data/ftrace_event.proto"
    ))
    .expect("ftrace event proto source can be read");
    let trace_result = fs::read_to_string(format!(
        "{manifest_dir}/proto/ftrace_data/trace_plugin_result.proto"
    ))
    .expect("trace plugin result proto source can be read");

    assert!(profiler_plugin_data.contains("message ProfilerPluginData"));
    assert!(profiler_plugin_data.contains("enum ClockId"));
    assert!(!profiler_plugin_data.contains("message TracePluginResult"));
    assert!(ftrace_event.contains("message FtraceEvent"));
    assert!(ftrace_event.contains("import \"ftrace_data/sched.proto\";"));
    assert!(trace_result.contains("message TracePluginResult"));
    assert!(trace_result.contains("import \"ftrace_data/ftrace_event.proto\";"));
}

#[test]
fn sched_proto_uses_hitrace_package_and_project_format() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source = fs::read_to_string(format!("{manifest_dir}/proto/ftrace_data/sched.proto"))
        .expect("sched proto source can be read");

    assert!(source.contains("package kat.hitrace;"));
    assert!(source.contains("// Adapted from trace_streamer ftrace_data/sched.proto."));
    assert!(!source.contains("THIS FILE IS GENERATED BY"));
    assert!(source.contains("  int32 pid = 1;"));
}
