use prost::Message;
use std::fs;

#[allow(dead_code)]
mod proto {
    pub mod kat {
        pub mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }
    }

    pub(crate) use kat::hitrace::ProfilerPluginData;
}

mod catalog {
    #![allow(dead_code)]

    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/catalog.rs"));
}

mod domains {
    pub(crate) mod ftrace {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/domains/ftrace/event.rs"
        ));
    }
}

mod sinks {
    pub(crate) mod arrow {
        #[allow(dead_code)]
        mod table_builder {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/sinks/arrow/table_builder.rs"
            ));
        }

        pub(crate) use table_builder::{DirectEventTableBuilder, EventMeta};
    }
}

mod ftrace_event_table_builders {
    include!(concat!(env!("OUT_DIR"), "/ftrace_event_table_builders.rs"));
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
fn generated_ftrace_event_table_builders_route_direct_events_to_tables() {
    let mut builders =
        ftrace_event_table_builders::FtraceEventTableBuilders::new().expect("builders are created");

    builders
        .push_event(domains::ftrace::FtraceEventRecord::new(
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
        ))
        .expect("event is routed");

    let tables = builders.into_tables().expect("tables are built");
    let sched_switch = tables
        .iter()
        .find(|table| table.name == "sched_switch")
        .expect("sched_switch table exists");

    assert_eq!(sched_switch.batches[0].num_rows(), 1);
}

#[test]
fn direct_event_table_builder_combines_meta_and_message_fields() {
    let event = proto::kat::hitrace::FtraceEvent {
        timestamp: 20,
        tgid: 500,
        comm: "source".to_string(),
        ..Default::default()
    };
    let record = domains::ftrace::FtraceEventRecord::new(3, event);
    let meta = sinks::arrow::EventMeta::from_record(&record);
    let mut builder = sinks::arrow::DirectEventTableBuilder::new::<
        proto::kat::hitrace::SchedSwitchFormat,
    >("sched_switch")
    .expect("builder is created from meta and message schemas");

    builder
        .push(
            meta,
            proto::kat::hitrace::SchedSwitchFormat {
                prev_comm: "render".to_string(),
                prev_pid: 42,
                prev_prio: 120,
                prev_state: 1,
                next_comm: "main".to_string(),
                next_pid: 7,
                next_prio: 100,
            },
        )
        .expect("event row is appended");

    let table = builder.into_table().expect("table is built");
    let batch = &table.batches[0];
    let schema = batch.schema();

    assert_eq!(batch.num_rows(), 1);
    for field in [
        "event_timestamp",
        "event_cpu",
        "event_tgid",
        "event_comm",
        "prev_comm",
        "prev_pid",
        "prev_prio",
        "prev_state",
        "next_comm",
        "next_pid",
        "next_prio",
    ] {
        assert!(
            schema.field_with_name(field).is_ok(),
            "{field} should be a top-level column"
        );
    }
    assert!(schema.field_with_name("meta").is_err());
    assert!(schema.field_with_name("message").is_err());
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
