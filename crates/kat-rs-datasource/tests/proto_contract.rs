use prost::Message;
use std::fs;

#[allow(dead_code)]
mod proto {
    pub mod kat {
        pub mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }
    }
}

mod sched_rows {
    include!(concat!(env!("OUT_DIR"), "/sched_rows.rs"));
}

mod hitrace {
    use arrow_array::RecordBatch;

    pub(crate) struct HitraceTable {
        pub(crate) name: &'static str,
        pub(crate) batches: Vec<RecordBatch>,
    }

    mod table_builder {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/hitrace/table_builder.rs"
        ));
    }

    pub(crate) use table_builder::TableBuilder;
}

mod sched_table_builders {
    include!(concat!(env!("OUT_DIR"), "/sched_table_builders.rs"));
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
    assert_eq!(decoded.prev_prio, 120);
    assert_eq!(decoded.prev_state, 1);
    assert_eq!(decoded.next_comm, "main");
    assert_eq!(decoded.next_pid, 7);
    assert_eq!(decoded.next_prio, 100);
}

#[test]
fn generated_proto_includes_upstream_sched_messages() {
    let value = proto::kat::hitrace::SchedBlockedReasonFormat {
        pid: 42,
        caller: 0xfeed_beef,
        io_wait: 1,
    };

    let decoded =
        proto::kat::hitrace::SchedBlockedReasonFormat::decode(value.encode_to_vec().as_slice())
            .expect("decode");

    assert_eq!(decoded.pid, 42);
    assert_eq!(decoded.caller, 0xfeed_beef);
    assert_eq!(decoded.io_wait, 1);
}

#[test]
fn generated_ftrace_event_uses_direct_sched_fields() {
    let value = proto::kat::hitrace::FtraceEvent {
        timestamp: 10,
        tgid: 500,
        comm: "source".to_string(),
        sched_switch_format: Some(proto::kat::hitrace::SchedSwitchFormat {
            prev_comm: "render".to_string(),
            prev_pid: 42,
            prev_prio: 120,
            prev_state: 1,
            next_comm: "main".to_string(),
            next_pid: 7,
            next_prio: 100,
        }),
        sched_blocked_reason_format: Some(proto::kat::hitrace::SchedBlockedReasonFormat {
            pid: 42,
            caller: 0xfeed_beef,
            io_wait: 1,
        }),
        ..Default::default()
    };

    let decoded =
        proto::kat::hitrace::FtraceEvent::decode(value.encode_to_vec().as_slice()).expect("decode");

    assert_eq!(decoded.timestamp, 10);
    assert!(decoded.sched_switch_format.is_some());
    assert!(decoded.sched_blocked_reason_format.is_some());
}

#[test]
fn generated_sched_rows_include_event_metadata_and_message_fields() {
    let meta = sched_rows::SchedEventMeta {
        event_timestamp: 20,
        event_cpu: 3,
        event_tgid: 500,
        event_comm: "source".to_string(),
    };

    let row = sched_rows::SchedProcessWaitRow::new(
        &meta,
        proto::kat::hitrace::SchedProcessWaitFormat {
            comm: "RenderThread".to_string(),
            pid: 42,
            prio: 120,
        },
    );

    assert_eq!(
        sched_rows::SchedProcessWaitRow::TABLE_NAME,
        "sched_process_wait"
    );
    assert_eq!(row.event_timestamp, 20);
    assert_eq!(row.event_cpu, 3);
    assert_eq!(row.event_tgid, 500);
    assert_eq!(row.event_comm, "source");
    assert_eq!(row.comm, "RenderThread");
    assert_eq!(row.pid, 42);
    assert_eq!(row.prio, 120);
}

#[test]
fn generated_sched_table_builders_route_direct_events_to_tables() {
    let mut builders =
        sched_table_builders::SchedDirectTableBuilders::new().expect("builders are created");

    builders
        .push_event(
            3,
            proto::kat::hitrace::FtraceEvent {
                timestamp: 20,
                tgid: 500,
                comm: "source".to_string(),
                sched_switch_format: Some(proto::kat::hitrace::SchedSwitchFormat {
                    prev_comm: "render".to_string(),
                    prev_pid: 42,
                    prev_prio: 120,
                    prev_state: 1,
                    next_comm: "main".to_string(),
                    next_pid: 7,
                    next_prio: 100,
                }),
                ..Default::default()
            },
        )
        .expect("event is routed");

    let tables = builders.into_tables().expect("tables are built");
    let sched_switch = tables
        .iter()
        .find(|table| table.name == "sched_switch")
        .expect("sched_switch table exists");

    assert_eq!(sched_switch.batches[0].num_rows(), 1);
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
    assert!(!source.contains("   int32 pid = 1;"));
}
