use prost::Message;

use kat_datasource as kat_rs_datasource;

#[test]
fn generated_descriptor_summary_contains_current_proto_roots() {
    let roots = kat_rs_datasource::relational_for_tests::descriptor_root_names();

    for root in [
        "CpuData",
        "MemoryData",
        "ProcessData",
        "DiskioData",
        "NetworkDatas",
        "GpuData",
        "TracePluginResult",
        "FtraceCpuDetailMsg",
        "FtraceEvent",
        "BinderCommandFormat",
        "BlockRqIssueFormat",
        "BatchNativeHookData",
        "NativeHookData",
        "AllocEvent",
        "Frame",
    ] {
        assert!(
            roots.contains(&root.to_string()),
            "{root} should be available to relational planning"
        );
    }
}

#[test]
fn expansion_plan_detects_tables_from_selected_roots() {
    let table_names = kat_rs_datasource::relational_for_tests::expansion_plan_table_names(&[
        "MemoryData",
        "TracePluginResult",
        "NativeHookConfig",
        "BatchNativeHookData",
    ]);

    for table in [
        "memory_data",
        "memory_data__processesinfo",
        "memory_data__processesinfo__smapinfo",
        "trace_plugin_result__ftrace_cpu_detail",
        "trace_plugin_result__ftrace_cpu_detail__event",
        "trace_plugin_result__ftrace_cpu_detail__event__sched_switch_format",
        "batch_native_hook_data__events",
        "batch_native_hook_data__events__alloc_event",
        "batch_native_hook_data__events__alloc_event__frame_info",
        "batch_native_hook_data__events__stack_map__frame_map_id",
        "batch_native_hook_data__events__stack_map__ip",
        "native_hook_config__expand_pids",
        "native_hook_config__restrace_tag",
    ] {
        assert!(
            table_names.contains(&table.to_string()),
            "{table} should be derived from selected roots"
        );
    }

    let cpu_tables =
        kat_rs_datasource::relational_for_tests::expansion_plan_table_names(&["CpuData"]);
    assert!(cpu_tables.contains(&"cpu_data".to_string()));
    assert!(
        !cpu_tables.contains(&"memory_data".to_string()),
        "unselected roots should not leak into the plan"
    );
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

        pub mod cpu_data {
            include!(concat!(env!("OUT_DIR"), "/kat.cpu_data.rs"));
        }

        pub mod memory_data {
            include!(concat!(env!("OUT_DIR"), "/kat.memory_data.rs"));
        }

        pub mod process_data {
            include!(concat!(env!("OUT_DIR"), "/kat.process_data.rs"));
        }

        pub mod diskio_data {
            include!(concat!(env!("OUT_DIR"), "/kat.diskio_data.rs"));
        }

        pub mod network_data {
            include!(concat!(env!("OUT_DIR"), "/kat.network_data.rs"));
        }

        pub mod gpu_data {
            include!(concat!(env!("OUT_DIR"), "/kat.gpu_data.rs"));
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
    assert_eq!(decoded.prev_prio, 120);
    assert_eq!(decoded.prev_state, 1);
    assert_eq!(decoded.next_comm, "main");
    assert_eq!(decoded.next_pid, 7);
    assert_eq!(decoded.next_prio, 100);
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
    assert_eq!(decoded.common_fields.expect("common fields decode").pid, 42);
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
    assert_eq!(decoded.expand_pids, vec![42, 77]);
    assert!(decoded.dump_nmd);

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
                        thread_name_id: 7,
                        stack_id: 8,
                        ..Default::default()
                    },
                ),
            ),
        }],
    };

    let decoded =
        proto::kat::native_hook::BatchNativeHookData::decode(batch.encode_to_vec().as_slice())
            .expect("decode");

    assert_eq!(decoded.events.len(), 1);
    assert!(matches!(
        decoded.events[0].event,
        Some(proto::kat::native_hook::native_hook_data::Event::AllocEvent(_))
    ));
}

#[test]
fn generated_proto_includes_fixed_result_system_plugins() {
    let cpu = proto::kat::cpu_data::CpuData {
        process_num: 2,
        user_load: 1.5,
        sys_load: 2.5,
        total_load: 4.0,
        ..Default::default()
    };
    let decoded =
        proto::kat::cpu_data::CpuData::decode(cpu.encode_to_vec().as_slice()).expect("decode");
    assert_eq!(decoded.process_num, 2);
    assert_eq!(decoded.user_load, 1.5);
    assert_eq!(decoded.total_load, 4.0);

    let memory = proto::kat::memory_data::MemoryData {
        zram: 64,
        gpu_limit_size: 128,
        gpu_used_size: 32,
        ..Default::default()
    };
    let decoded = proto::kat::memory_data::MemoryData::decode(memory.encode_to_vec().as_slice())
        .expect("decode");
    assert_eq!(decoded.zram, 64);
    assert_eq!(decoded.gpu_limit_size, 128);
    assert_eq!(decoded.gpu_used_size, 32);

    let process = proto::kat::process_data::ProcessData {
        processesinfo: vec![proto::kat::process_data::ProcessInfo {
            pid: 42,
            name: "render".to_string(),
            ppid: 7,
            uid: 1000,
            cpuinfo: Some(proto::kat::process_data::CpuInfo {
                cpu_usage: 12.5,
                thread_sum: 3,
                cpu_time_ms: 456,
            }),
            ..Default::default()
        }],
    };
    let decoded = proto::kat::process_data::ProcessData::decode(process.encode_to_vec().as_slice())
        .expect("decode");
    assert_eq!(decoded.processesinfo.len(), 1);
    assert_eq!(decoded.processesinfo[0].pid, 42);

    let diskio = proto::kat::diskio_data::DiskioData {
        rd_sectors_kb: 10,
        wr_sectors_kb: 20,
        ..Default::default()
    };
    let decoded = proto::kat::diskio_data::DiskioData::decode(diskio.encode_to_vec().as_slice())
        .expect("decode");
    assert_eq!(decoded.rd_sectors_kb, 10);

    let network = proto::kat::network_data::NetworkDatas {
        networkinfo: vec![proto::kat::network_data::NetworkData {
            pid: 42,
            tx_bytes: 100,
            rx_bytes: 200,
            ..Default::default()
        }],
        ..Default::default()
    };
    let decoded =
        proto::kat::network_data::NetworkDatas::decode(network.encode_to_vec().as_slice())
            .expect("decode");
    assert_eq!(decoded.networkinfo.len(), 1);

    let gpu = proto::kat::gpu_data::GpuData {
        boottime: 100,
        gpu_utilisation: 80,
        gpu_data_array: vec![proto::kat::gpu_data::GpuDataExt {
            boottime: 101,
            gpu_utilisation: 81,
        }],
    };
    let decoded =
        proto::kat::gpu_data::GpuData::decode(gpu.encode_to_vec().as_slice()).expect("decode");
    assert_eq!(decoded.boottime, 100);
    assert_eq!(decoded.gpu_data_array.len(), 1);
}
