use std::fs;

use arrow_array::types::{Int32Type, UInt32Type, UInt64Type};
use arrow_schema::DataType;
use prost::Message;
use tempfile::tempdir;

#[path = "support/mod.rs"]
mod support;
use support::{Relation, assert_no_staging, profiler_section};

#[allow(dead_code)]
mod proto {
    pub mod kat {
        pub mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }
    }
}

use proto::kat::hitrace::{
    ClockDetailMsg, FtraceCpuDetailMsg, FtraceCpuStatsMsg, FtraceEvent, IrqHandlerEntryFormat,
    PerCpuStatsMsg, ProfilerPluginData, SchedSwitchFormat, TracePluginConfig, TracePluginResult,
    clock_detail_msg, ftrace_cpu_stats_msg, ftrace_event, trace_plugin_config,
};

#[test]
fn decode_preserves_ftrace_values_presence_oneof_parentage_and_repeated_order() {
    let root = tempdir().expect("temporary decode directory is created");
    let source = root.path().join("ftrace.htrace");
    let destination = root.path().join("relations");
    let result = representative_result();
    let config = representative_config();
    fs::write(
        &source,
        profiler_section([
            profiler_envelope("ftrace-plugin", 31, result.encode_to_vec()),
            profiler_envelope("ftrace-plugin_config", 32, config.encode_to_vec()),
        ]),
    )
    .expect("typed ftrace fixture is written");

    let report = kat_datasource::decode_hitrace(&source, &destination)
        .expect("decode publishes descriptor-derived Ftrace relations");

    assert!(report.unsupported_plugins().is_empty());
    assert!(!destination.join("sched_switch.parquet").exists());
    for relation in [
        "profiler_payload_occurrence",
        "trace_plugin_result",
        "trace_plugin_result_ftrace_cpu_detail",
        "trace_plugin_result_ftrace_cpu_detail_event",
        "trace_plugin_result_ftrace_cpu_detail_event_sched_switch_format",
        "trace_plugin_result_ftrace_cpu_detail_event_irq_handler_entry_format",
        "trace_plugin_result_clocks_detail",
        "trace_plugin_config",
        "trace_plugin_config_ftrace_events",
        "protobuf_enum_symbol",
    ] {
        assert!(
            destination.join(format!("{relation}.parquet")).is_file(),
            "expected descriptor relation {relation:?}"
        );
    }

    let occurrences = Relation::open(&destination, "profiler_payload_occurrence");
    assert_eq!(
        occurrences.string_values("envelope_name"),
        [
            Some("ftrace-plugin".to_owned()),
            Some("ftrace-plugin_config".to_owned()),
        ]
    );
    assert_eq!(
        occurrences.primitive_values::<UInt32Type>("status"),
        [Some(31), Some(32)]
    );

    let result_root = Relation::open(&destination, "trace_plugin_result");
    let config_root = Relation::open(&destination, "trace_plugin_config");
    assert_eq!(
        result_root.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        [Some(0)]
    );
    assert_eq!(
        config_root.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        [Some(1)]
    );
    assert_eq!(
        config_root.string_values("clock"),
        [Some("boot".to_owned())]
    );
    assert_eq!(
        config_root.primitive_values::<Int32Type>("parse_mode"),
        [Some(trace_plugin_config::ParseMode::RawData as i32)]
    );

    let details = Relation::open(&destination, "trace_plugin_result_ftrace_cpu_detail");
    assert_eq!(
        details.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        [Some(0)]
    );
    assert_eq!(
        details.primitive_values::<UInt64Type>("_kat_repeated_index"),
        [Some(0)]
    );
    assert_eq!(details.primitive_values::<UInt32Type>("cpu"), [Some(7)]);

    let events = Relation::open(&destination, "trace_plugin_result_ftrace_cpu_detail_event");
    assert_eq!(
        events.primitive_values::<UInt64Type>("_kat_row_id"),
        [Some(0), Some(1)]
    );
    assert_eq!(
        events.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        [Some(0), Some(0)]
    );
    assert_eq!(
        events.primitive_values::<UInt64Type>("_kat_repeated_index"),
        [Some(0), Some(1)]
    );
    assert_eq!(
        events.primitive_values::<UInt64Type>("timestamp"),
        [Some(101), Some(102)]
    );
    assert_eq!(
        events.string_values("comm"),
        [Some("first".to_owned()), Some("second".to_owned())]
    );
    let common_fields = events
        .schema()
        .field_with_name("common_fields")
        .expect("event schema has common_fields");
    assert!(common_fields.is_nullable());
    let DataType::Struct(children) = common_fields.data_type() else {
        panic!("common_fields must remain an Arrow Struct")
    };
    assert!(children.iter().all(|child| child.is_nullable()));
    assert_eq!(events.struct_nulls("common_fields"), [true, false]);
    assert_eq!(
        events.struct_primitive_values::<Int32Type>("common_fields", "pid"),
        [None, Some(44)]
    );

    let switches = Relation::open(
        &destination,
        "trace_plugin_result_ftrace_cpu_detail_event_sched_switch_format",
    );
    assert_eq!(
        switches.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        [Some(0)]
    );
    assert_eq!(
        switches.string_values("prev_comm"),
        [Some("before".to_owned())]
    );
    assert_eq!(
        switches.primitive_values::<UInt64Type>("prev_state"),
        [Some(0x1_0000_0001)]
    );

    let irq = Relation::open(
        &destination,
        "trace_plugin_result_ftrace_cpu_detail_event_irq_handler_entry_format",
    );
    assert_eq!(
        irq.primitive_values::<UInt64Type>("_kat_parent_row_id"),
        [Some(1)]
    );
    assert_eq!(irq.string_values("name"), [Some("irq-name".to_owned())]);

    let clocks = Relation::open(&destination, "trace_plugin_result_clocks_detail");
    assert_eq!(
        clocks.primitive_values::<UInt64Type>("_kat_repeated_index"),
        [Some(0), Some(1)]
    );
    assert_eq!(
        clocks.primitive_values::<Int32Type>("id"),
        [
            Some(clock_detail_msg::ClockId::Boottime as i32),
            Some(clock_detail_msg::ClockId::MonotonicRaw as i32),
        ]
    );
    assert_eq!(clocks.struct_nulls("resolution"), [true, false]);

    let configured_events = Relation::open(&destination, "trace_plugin_config_ftrace_events");
    assert_eq!(
        configured_events.primitive_values::<UInt64Type>("_kat_repeated_index"),
        [Some(0), Some(1)]
    );
    assert_eq!(
        configured_events.string_values("value"),
        [
            Some("sched/sched_switch".to_owned()),
            Some("irq/irq_handler_entry".to_owned()),
        ]
    );

    let symbols = Relation::open(&destination, "protobuf_enum_symbol");
    let origin_tables = symbols.string_values("origin_table");
    let origin_fields = symbols.string_values("origin_field_path");
    let enum_numbers = symbols.primitive_values::<Int32Type>("enum_number");
    let enum_symbols = symbols.string_values("enum_symbol");
    assert!((0..symbols.row_count()).any(|index| {
        origin_tables[index].as_deref() == Some("trace_plugin_config")
            && origin_fields[index].as_deref() == Some("parse_mode")
            && enum_numbers[index] == Some(trace_plugin_config::ParseMode::RawData as i32)
            && enum_symbols[index].as_deref() == Some("RAW_DATA")
    }));
}

#[test]
fn malformed_bound_ftrace_payload_is_terminal_and_leaves_no_output() {
    let root = tempdir().expect("temporary decode directory is created");
    let source = root.path().join("malformed-ftrace.htrace");
    let destination = root.path().join("relations");
    fs::write(
        &source,
        profiler_section([profiler_envelope("ftrace-plugin", 1, vec![0x80])]),
    )
    .expect("malformed bound fixture is written");

    let error = kat_datasource::decode_hitrace(&source, &destination)
        .expect_err("a malformed payload on an exact route is rejected");

    assert!(
        error
            .to_string()
            .contains("failed to decode ftrace-plugin payload"),
        "unexpected error: {error:#}"
    );
    assert!(!destination.exists());
    assert_no_staging(root.path());
}

fn representative_result() -> TracePluginResult {
    TracePluginResult {
        ftrace_cpu_stats: vec![
            FtraceCpuStatsMsg {
                status: ftrace_cpu_stats_msg::Status::TraceStart as i32,
                per_cpu_stats: vec![PerCpuStatsMsg {
                    cpu: 7,
                    ..Default::default()
                }],
                trace_clock: "boot".to_owned(),
            },
            FtraceCpuStatsMsg {
                status: ftrace_cpu_stats_msg::Status::TraceEnd as i32,
                per_cpu_stats: vec![PerCpuStatsMsg {
                    cpu: 7,
                    ..Default::default()
                }],
                trace_clock: "boot".to_owned(),
            },
        ],
        ftrace_cpu_detail: vec![FtraceCpuDetailMsg {
            cpu: 7,
            overwrite: 0,
            event: vec![
                FtraceEvent {
                    timestamp: 101,
                    tgid: 201,
                    comm: "first".to_owned(),
                    common_fields: None,
                    event: Some(ftrace_event::Event::SchedSwitchFormat(SchedSwitchFormat {
                        prev_comm: "before".to_owned(),
                        prev_pid: 301,
                        prev_prio: 11,
                        prev_state: 0x1_0000_0001,
                        next_comm: "after".to_owned(),
                        next_pid: 302,
                        next_prio: 12,
                    })),
                },
                FtraceEvent {
                    timestamp: 102,
                    tgid: 202,
                    comm: "second".to_owned(),
                    common_fields: Some(ftrace_event::CommonFileds {
                        r#type: 41,
                        flags: 42,
                        preempt_count: 43,
                        pid: 44,
                    }),
                    event: Some(ftrace_event::Event::IrqHandlerEntryFormat(
                        IrqHandlerEntryFormat {
                            irq: 55,
                            name: "irq-name".to_owned(),
                        },
                    )),
                },
            ],
        }],
        clocks_detail: vec![
            ClockDetailMsg {
                id: clock_detail_msg::ClockId::Boottime as i32,
                time: Some(clock_detail_msg::TimeSpec {
                    tv_sec: 601,
                    tv_nsec: 602,
                }),
                resolution: None,
            },
            ClockDetailMsg {
                id: clock_detail_msg::ClockId::MonotonicRaw as i32,
                time: Some(clock_detail_msg::TimeSpec {
                    tv_sec: 605,
                    tv_nsec: 606,
                }),
                resolution: Some(clock_detail_msg::TimeSpec {
                    tv_sec: 603,
                    tv_nsec: 604,
                }),
            },
        ],
        version: "ftrace-v1".to_owned(),
        ..Default::default()
    }
}

fn representative_config() -> TracePluginConfig {
    TracePluginConfig {
        ftrace_events: vec![
            "sched/sched_switch".to_owned(),
            "irq/irq_handler_entry".to_owned(),
        ],
        hitrace_categories: vec!["sched".to_owned(), "irq".to_owned()],
        hitrace_apps: vec!["app-b".to_owned(), "app-a".to_owned()],
        buffer_size_kb: 8_192,
        clock: "boot".to_owned(),
        parse_mode: trace_plugin_config::ParseMode::RawData as i32,
        ..Default::default()
    }
}

fn profiler_envelope(name: &str, status: u32, data: Vec<u8>) -> ProfilerPluginData {
    ProfilerPluginData {
        name: name.to_owned(),
        status,
        data,
        clock_id: 7,
        tv_sec: 100 + u64::from(status),
        tv_nsec: 200 + u64::from(status),
        version: format!("route-{status}"),
        sample_interval: status,
    }
}
