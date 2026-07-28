use prost::Message;
use std::{collections::BTreeSet, fs};

mod relational {
    pub(crate) mod descriptor {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/relational/descriptor.rs"
        ));
    }

    pub(crate) mod rules {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/relational/rules.rs"
        ));
    }

    pub(crate) mod plan {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/relational/plan.rs"
        ));
    }
}

#[test]
fn relational_descriptor_contains_current_profiler_roots() {
    for root in [
        "CpuData",
        "MemoryData",
        "ProcessData",
        "DiskioData",
        "NetworkDatas",
        "GpuData",
        "TracePluginResult",
        "BatchNativeHookData",
        "NativeHookData",
        "AllocEvent",
        "Frame",
    ] {
        assert!(
            relational::descriptor::RELATIONAL_DESCRIPTORS
                .iter()
                .any(|candidate| candidate.name == root),
            "{root} should be available to relational planning"
        );
    }
}

#[test]
fn relational_plan_derives_current_root_repeated_and_oneof_tables() {
    let plan = relational::plan::expansion_plan_for_roots(&[
        "MemoryData",
        "TracePluginResult",
        "NativeHookConfig",
        "BatchNativeHookData",
    ])
    .expect("registered roots should produce a relational plan");
    let tables = plan
        .iter()
        .map(|item| item.output_table.as_str())
        .collect::<Vec<_>>();

    for table in [
        "memory_data",
        "memory_data_processesinfo",
        "memory_data_processesinfo_smapinfo",
        "trace_plugin_result",
        "trace_plugin_result_ftrace_cpu_detail",
        "trace_plugin_result_ftrace_cpu_detail_event",
        "native_hook_config",
        "native_hook_config_expand_pids",
        "batch_native_hook_data",
        "batch_native_hook_data_events",
        "batch_native_hook_data_events_alloc_event",
        "batch_native_hook_data_events_alloc_event_frame_info",
        "batch_native_hook_data_events_stack_map",
        "batch_native_hook_data_events_stack_map_ip",
    ] {
        assert!(
            tables.contains(&table),
            "{table} should be derived from the selected roots"
        );
    }
    assert!(
        tables.iter().all(|table| !table.contains("__")),
        "Dataset Storage table names use canonical single underscores"
    );
    assert!(
        !tables.contains(&"batch_native_hook_data_events_event"),
        "a oneof group is not a physical table"
    );

    for relationship in [
        (
            "batch_native_hook_data_events_alloc_event",
            Some("batch_native_hook_data_events"),
        ),
        (
            "batch_native_hook_data_events_alloc_event_frame_info",
            Some("batch_native_hook_data_events_alloc_event"),
        ),
    ] {
        assert!(
            plan.iter().any(|item| {
                item.output_table == relationship.0
                    && item.parent_table.as_deref() == relationship.1
            }),
            "{relationship:?} should use an existing physical parent table"
        );
    }
}

#[test]
fn relational_plan_rejects_an_unknown_root() {
    let error = relational::plan::expansion_plan_for_roots(&["MissingProfilerRoot"])
        .expect_err("an unknown profiler root must not be silently skipped");

    assert!(
        error
            .to_string()
            .contains("missing relational root message descriptor: MissingProfilerRoot")
    );
}

#[test]
fn generated_bytes_are_binary_and_repeated_numbers_remain_arrays() {
    let symbol_table = proto::kat::native_hook::SymbolTable {
        sym_table: vec![0x00, 0xff, 0x41],
        str_table: vec![0x10, 0x20],
        ..Default::default()
    };
    let symbol_value =
        payload_value::to_payload_value(&symbol_table).expect("symbol table serializes");
    let symbol_fields = symbol_value.as_object().expect("symbol table is an object");

    for (name, expected) in [
        ("sym_table", &[0x00, 0xff, 0x41][..]),
        ("str_table", &[0x10, 0x20][..]),
    ] {
        let value = symbol_fields
            .iter()
            .find(|field| field.name() == name)
            .unwrap_or_else(|| panic!("{name} should be serialized"));
        assert_eq!(value.value().as_binary(), Some(expected));
    }

    let stack_map = proto::kat::native_hook::StackMap {
        ip: vec![0x1000, 0x2000],
        ..Default::default()
    };
    let stack_value = payload_value::to_payload_value(&stack_map).expect("stack map serializes");
    let ip = stack_value
        .as_object()
        .expect("stack map is an object")
        .iter()
        .find(|field| field.name() == "ip")
        .expect("ip should be serialized");
    assert_eq!(ip.value().as_array().map(<[_]>::len), Some(2));
    assert!(ip.value().as_binary().is_none());
}

#[test]
fn registered_payload_roots_have_relational_plans() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let route_files = [
        "src/decode/profiler/fixed_result/mod.rs",
        "src/decode/profiler/ftrace/mod.rs",
        "src/decode/profiler/native_hook/mod.rs",
    ];
    let mut roots = BTreeSet::new();

    for relative_path in route_files {
        let source = fs::read_to_string(format!("{manifest_dir}/{relative_path}"))
            .unwrap_or_else(|error| panic!("failed to read {relative_path}: {error}"));
        roots.extend(
            source
                .lines()
                .filter_map(route_root_message)
                .map(str::to_owned),
        );
    }

    assert!(
        !roots.is_empty(),
        "profiler routes should declare payload roots"
    );
    for root in roots {
        let plan =
            relational::plan::expansion_plan_for_roots(&[root.as_str()]).unwrap_or_else(|error| {
                panic!("registered payload root {root} must have a relational plan: {error:#}")
            });
        assert!(
            !plan.is_empty(),
            "registered payload root {root} must produce at least one table"
        );
    }
}

fn route_root_message(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix("root_message: \"")?
        .strip_suffix("\",")
}

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

    pub(crate) use kat::hitrace::ProfilerPluginData;
    pub(crate) use kat::native_hook::NativeHookConfig;
}

mod arrow_table {
    #![allow(dead_code)]

    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/arrow_table.rs"));
}

#[allow(dead_code)]
mod payload_value {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/payload_value.rs"));
}

mod record {
    #![allow(dead_code)]

    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/record.rs"));
}

mod domains {
    pub(crate) mod ftrace {
        #![allow(dead_code)]

        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/domains/ftrace/event.rs"
        ));
    }

    pub(crate) mod native_hook {
        #![allow(dead_code)]

        mod event {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/domains/native_hook/event.rs"
            ));
        }

        mod records {
            include!(concat!(env!("OUT_DIR"), "/native_hook_records.rs"));
        }

        pub(crate) use event::{NativeHookEvent, NativeHookEventContext};
        pub(crate) use records::NativeHookRecord;
    }
}

mod sinks {
    pub(crate) mod arrow {
        #[allow(dead_code)]
        mod table {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/sinks/arrow/table.rs"
            ));
        }

        #[allow(dead_code)]
        mod ftrace {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/sinks/arrow/ftrace.rs"
            ));
        }

        pub(crate) use ftrace::{EventMeta, FtraceEventTableBuilder};
    }
}

mod ftrace_event_table_builders {
    include!(concat!(env!("OUT_DIR"), "/ftrace_event_table_builders.rs"));
}

#[test]
fn trace_record_stream_models_pre_sink_records() {
    let clock_id =
        proto::kat::hitrace::profiler_plugin_data::ClockId::from_str_name("CLOCKID_BOOTTIME")
            .expect("upstream clock id enum exists");
    let plugin = proto::ProfilerPluginData {
        name: "ftrace-plugin".to_string(),
        clock_id: clock_id as i32,
        ..Default::default()
    };

    match record::TraceRecord::ProfilerPluginData(plugin) {
        record::TraceRecord::ProfilerPluginData(record) => {
            assert_eq!(record.name, "ftrace-plugin");
            assert_eq!(
                proto::kat::hitrace::profiler_plugin_data::ClockId::try_from(record.clock_id)
                    .expect("clock id decodes")
                    .as_str_name(),
                "CLOCKID_BOOTTIME"
            );
        }
        record::TraceRecord::FtraceCapture(_) => unreachable!("expected plugin data record"),
        record::TraceRecord::Ftrace(_) => unreachable!("expected plugin data record"),
        record::TraceRecord::NativeHook(_) => unreachable!("expected plugin data record"),
        record::TraceRecord::DecodedPayload(_) => unreachable!("expected plugin data record"),
    }

    let event = domains::ftrace::FtraceEventRecord::new(
        3,
        proto::kat::hitrace::FtraceEvent {
            timestamp: 20,
            tgid: 500,
            comm: "source".to_string(),
            ..Default::default()
        },
    );

    match record::TraceRecord::Ftrace(Box::new(domains::ftrace::FtraceRecord::Event(Box::new(
        event,
    )))) {
        record::TraceRecord::Ftrace(record) => match *record {
            domains::ftrace::FtraceRecord::Event(event) => {
                assert_eq!(event.context.cpu, 3);
                assert_eq!(event.event.timestamp, 20);
            }
        },
        record::TraceRecord::FtraceCapture(_) => unreachable!("expected ftrace event record"),
        record::TraceRecord::ProfilerPluginData(_) => unreachable!("expected ftrace event record"),
        record::TraceRecord::NativeHook(_) => unreachable!("expected ftrace event record"),
        record::TraceRecord::DecodedPayload(_) => unreachable!("expected ftrace event record"),
    }

    let config = proto::NativeHookConfig {
        pid: 42,
        process_name: "native".to_string(),
        ..Default::default()
    };
    match record::TraceRecord::NativeHook(Box::new(domains::native_hook::NativeHookRecord::Config(
        Box::new(config),
    ))) {
        record::TraceRecord::NativeHook(record) => match *record {
            domains::native_hook::NativeHookRecord::Config(config) => {
                assert_eq!(config.pid, 42);
                assert_eq!(config.process_name, "native");
            }
            _ => unreachable!("expected native hook config"),
        },
        record::TraceRecord::FtraceCapture(_) => unreachable!("expected native hook config"),
        record::TraceRecord::ProfilerPluginData(_) => unreachable!("expected native hook config"),
        record::TraceRecord::Ftrace(_) => unreachable!("expected native hook config"),
        record::TraceRecord::DecodedPayload(_) => unreachable!("expected native hook config"),
    }

    let event = domains::native_hook::NativeHookEvent::new(
        domains::native_hook::NativeHookEventContext::new(1, 2),
        proto::kat::native_hook::AllocEvent {
            pid: 42,
            ..Default::default()
        },
    );
    match record::TraceRecord::NativeHook(Box::new(domains::native_hook::NativeHookRecord::Alloc(
        Box::new(event),
    ))) {
        record::TraceRecord::NativeHook(record) => match *record {
            domains::native_hook::NativeHookRecord::Alloc(event) => {
                assert_eq!(event.context.tv_sec, 1);
                assert_eq!(event.context.tv_nsec, 2);
                assert_eq!(event.event.pid, 42);
            }
            _ => unreachable!("expected native hook event"),
        },
        record::TraceRecord::FtraceCapture(_) => unreachable!("expected native hook event"),
        record::TraceRecord::ProfilerPluginData(_) => unreachable!("expected native hook event"),
        record::TraceRecord::Ftrace(_) => unreachable!("expected native hook event"),
        record::TraceRecord::DecodedPayload(_) => unreachable!("expected native hook event"),
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
            caller_str: "finish_task_switch".to_string(),
        }),
        common_fields: Some(proto::kat::hitrace::ftrace_event::CommonFileds {
            r#type: 123,
            flags: 1,
            preempt_count: 2,
            pid: 42,
        }),
        ..Default::default()
    };

    let decoded =
        proto::kat::hitrace::FtraceEvent::decode(value.encode_to_vec().as_slice()).expect("decode");

    assert_eq!(decoded.timestamp, 10);
    assert!(decoded.sched_switch_format.is_some());
    assert!(decoded.sched_blocked_reason_format.is_some());
    let common_fields = decoded.common_fields.expect("common fields decode");
    assert_eq!(common_fields.r#type, 123);
    assert_eq!(common_fields.pid, 42);
}

#[test]
fn generated_proto_includes_native_hook_config_and_events() {
    let config = proto::kat::native_hook::NativeHookConfig {
        pid: 42,
        save_file: true,
        file_name: "native-hook.bin".to_string(),
        process_name: "render".to_string(),
        statistics_interval: 5,
        clock: "boottime".to_string(),
        sample_interval: 10,
        expand_pids: vec![42, 77],
        filter_napi_name: "napi".to_string(),
        dump_nmd: true,
        target_so_name: "libark_jsruntime.so".to_string(),
        restrace_tag: vec!["fd".to_string(), "vm".to_string()],
        ..Default::default()
    };
    let decoded =
        proto::kat::native_hook::NativeHookConfig::decode(config.encode_to_vec().as_slice())
            .expect("decode");

    assert_eq!(decoded.pid, 42);
    assert!(decoded.save_file);
    assert_eq!(decoded.file_name, "native-hook.bin");
    assert_eq!(decoded.statistics_interval, 5);
    assert_eq!(decoded.expand_pids, vec![42, 77]);
    assert!(decoded.dump_nmd);
    assert_eq!(decoded.target_so_name, "libark_jsruntime.so");
    assert_eq!(decoded.restrace_tag, vec!["fd", "vm"]);

    let batch =
        proto::kat::native_hook::BatchNativeHookData {
            events: vec![
            proto::kat::native_hook::NativeHookData {
                tv_sec: 1,
                tv_nsec: 20,
                event: Some(proto::kat::native_hook::native_hook_data::Event::AllocEvent(
                    proto::kat::native_hook::AllocEvent {
                        pid: 42,
                        tid: 43,
                        addr: 0x1000,
                        size: 64,
                        thread_name_id: 7,
                        stack_id: 8,
                        ..Default::default()
                    },
                )),
            },
            proto::kat::native_hook::NativeHookData {
                tv_sec: 2,
                tv_nsec: 30,
                event: Some(
                    proto::kat::native_hook::native_hook_data::Event::StatisticsEvent(
                        proto::kat::native_hook::RecordStatisticsEvent {
                            pid: 42,
                            callstack_id: 9,
                            r#type:
                                proto::kat::native_hook::record_statistics_event::MemoryType::Mmap
                                    as i32,
                            apply_count: 3,
                            release_count: 1,
                            apply_size: 256,
                            release_size: 128,
                            tag_name: "ashmem".to_string(),
                        },
                    ),
                ),
            },
            proto::kat::native_hook::NativeHookData {
                tv_sec: 3,
                tv_nsec: 40,
                event: Some(
                    proto::kat::native_hook::native_hook_data::Event::TraceAllocEvent(
                        proto::kat::native_hook::TraceAllocEvent {
                            pid: 42,
                            tid: 44,
                            addr: 0x2000,
                            trace_type: proto::kat::native_hook::TraceType::Fd as i32,
                            tag_name: "fd".to_string(),
                            size: 16,
                            thread_name_id: 11,
                            stack_id: 12,
                            ..Default::default()
                        },
                    ),
                ),
            },
            proto::kat::native_hook::NativeHookData {
                tv_sec: 4,
                tv_nsec: 50,
                event: Some(
                    proto::kat::native_hook::native_hook_data::Event::TraceFreeEvent(
                        proto::kat::native_hook::TraceFreeEvent {
                            pid: 42,
                            tid: 44,
                            addr: 0x2000,
                            trace_type: proto::kat::native_hook::TraceType::Fd as i32,
                            tag_name: "fd".to_string(),
                            thread_name_id: 11,
                            stack_id: 12,
                            ..Default::default()
                        },
                    ),
                ),
            },
        ],
        };

    let decoded =
        proto::kat::native_hook::BatchNativeHookData::decode(batch.encode_to_vec().as_slice())
            .expect("decode");

    assert_eq!(decoded.events.len(), 4);
    assert_eq!(decoded.events[0].tv_sec, 1);
    assert!(matches!(
        decoded.events[0].event,
        Some(proto::kat::native_hook::native_hook_data::Event::AllocEvent(_))
    ));
    assert!(matches!(
        decoded.events[1].event,
        Some(proto::kat::native_hook::native_hook_data::Event::StatisticsEvent(_))
    ));
    assert!(matches!(
        decoded.events[2].event,
        Some(proto::kat::native_hook::native_hook_data::Event::TraceAllocEvent(_))
    ));
    assert!(matches!(
        decoded.events[3].event,
        Some(proto::kat::native_hook::native_hook_data::Event::TraceFreeEvent(_))
    ));
}

#[test]
fn generated_ftrace_table_set_routes_direct_events_to_tables() {
    let mut builders =
        ftrace_event_table_builders::FtraceTableSet::new().expect("builders are created");

    builders
        .push_record(domains::ftrace::FtraceRecord::Event(Box::new(
            domains::ftrace::FtraceEventRecord::new(
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
            ),
        )))
        .expect("event is routed");

    let tables = builders.into_tables().expect("tables are built");
    let sched_switch = tables
        .iter()
        .find(|table| table.name == "sched_switch")
        .expect("sched_switch table exists");

    assert_eq!(sched_switch.batches[0].num_rows(), 1);
}

#[test]
fn ftrace_event_table_builder_combines_meta_and_message_fields() {
    let event = proto::kat::hitrace::FtraceEvent {
        timestamp: 20,
        tgid: 500,
        comm: "source".to_string(),
        ..Default::default()
    };
    let record = domains::ftrace::FtraceEventRecord::new(3, event);
    let meta = sinks::arrow::EventMeta::from_record(&record);
    let mut builder = sinks::arrow::FtraceEventTableBuilder::new::<
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
    assert!(profiler_plugin_data.contains("services/common_types.proto::ProfilerPluginData"));
    assert!(!profiler_plugin_data.contains("message TracePluginResult"));
    assert!(!profiler_plugin_data.contains("message FtraceEvent"));
    assert!(ftrace_event.contains("message FtraceEvent"));
    assert!(ftrace_event.contains("message CommonFileds"));
    assert!(ftrace_event.contains("import \"ftrace_data/sched.proto\";"));
    assert!(trace_result.contains("message TracePluginResult"));
    assert!(trace_result.contains("repeated FtraceCpuStatsMsg ftrace_cpu_stats = 1;"));
    assert!(trace_result.contains("repeated ClockDetailMsg clocks_detail = 6;"));
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
    assert!(!source.contains("   int32 pid = 1;"));
}
