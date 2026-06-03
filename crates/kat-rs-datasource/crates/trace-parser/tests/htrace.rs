use arrow_array::{Array, StringArray};
use prost::Message;
use std::collections::BTreeSet;
use trace_model::ParsedTrace;
use trace_parser::plugins::{arkts, memory, process};
use trace_parser::{parsers::htrace::*, HarmonyTraceParser};

fn len_prefixed(plugin: ProfilerPluginData) -> Vec<u8> {
    let segment = plugin.encode_to_vec();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(segment.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&segment);
    bytes
}

fn append_segment(bytes: &mut Vec<u8>, plugin: ProfilerPluginData) {
    let segment = plugin.encode_to_vec();
    bytes.extend_from_slice(&(segment.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&segment);
}

fn assert_non_empty_tables_are_htrace_mvp_tables(parsed: &ParsedTrace) {
    let target_tables = BTreeSet::from([
        "trace_metadata",
        "trace_bounds",
        "process",
        "thread",
        "sched_slice",
        "thread_state",
        "raw_event",
        "raw",
        "instant",
        "irq",
        "measure",
        "measure_filter",
        "cpu_measure_filter",
        "symbols",
        "dma_fence",
        "cpu_usage",
        "diskio",
        "data_dict",
        "args",
        "callstack",
        "process_measure",
        "process_measure_filter",
        "sys_mem_measure",
        "sys_event_filter",
        "live_process",
        "js_heap_files",
        "js_heap_info",
        "js_heap_nodes",
        "js_heap_edges",
        "js_heap_string",
        "js_heap_location",
        "js_heap_sample",
        "js_heap_trace_function_info",
        "js_heap_trace_node",
        "js_config",
        "js_cpu_profiler_node",
        "js_cpu_profiler_sample",
    ]);

    for (name, batch) in parsed.tables.batches() {
        if batch.num_rows() > 0 {
            assert!(
                target_tables.contains(name),
                "{name} should not be emitted by the htrace MVP parser"
            );
        }
    }
}

#[test]
fn parses_len_prefixed_sched_switches() {
    let first = FtraceEvent {
        timestamp: 100,
        tgid: 1,
        comm: "prev".to_string(),
        common_fields: Some(FtraceEventCommonFields {
            event_type: 0,
            flags: 0,
            preempt_count: 0,
            pid: 1,
        }),
        sched_switch_format: Some(SchedSwitchFormat {
            prev_comm: "idle".to_string(),
            prev_pid: 0,
            prev_prio: 120,
            prev_state: 0,
            next_comm: "worker".to_string(),
            next_pid: 10,
            next_prio: 110,
        }),
        sched_wakeup_format: None,
        sched_wakeup_new_format: None,
        sched_waking_format: None,
        ..Default::default()
    };
    let second = FtraceEvent {
        timestamp: 150,
        tgid: 1,
        comm: "worker".to_string(),
        common_fields: Some(FtraceEventCommonFields {
            event_type: 0,
            flags: 0,
            preempt_count: 0,
            pid: 10,
        }),
        sched_switch_format: Some(SchedSwitchFormat {
            prev_comm: "worker".to_string(),
            prev_pid: 10,
            prev_prio: 110,
            prev_state: 1,
            next_comm: "idle".to_string(),
            next_pid: 0,
            next_prio: 120,
        }),
        sched_wakeup_format: None,
        sched_wakeup_new_format: None,
        sched_waking_format: None,
        ..Default::default()
    };

    let trace = TracePluginResult {
        ftrace_cpu_stats: vec![FtraceCpuStatsMsg {
            trace_clock: "boot".to_string(),
        }],
        ftrace_cpu_detail: vec![FtraceCpuDetailMsg {
            cpu: 0,
            event: vec![first, second],
            overwrite: 0,
        }],
        symbols_detail: vec![],
        clocks_detail: vec![],
    };

    let plugin = ProfilerPluginData {
        name: "ftrace-plugin".to_string(),
        status: 0,
        data: trace.encode_to_vec(),
        clock_id: 7,
        tv_sec: 0,
        tv_nsec: 0,
        version: "1.01".to_string(),
        sample_interval: 0,
    };

    let bytes = len_prefixed(plugin);

    let parsed = HtraceParser::default().parse_bytes(&bytes).unwrap();
    assert_non_empty_tables_are_htrace_mvp_tables(&parsed);
    assert_eq!(parsed.clock_domain, "boottime");
    assert_eq!(parsed.tables.sched_slice.num_rows(), 2);
    assert_eq!(parsed.tables.thread.num_rows(), 2);
}

#[test]
fn sorts_ftrace_events_across_cpu_details_before_filtering() {
    let switch = FtraceEvent {
        timestamp: 200,
        tgid: 1,
        comm: "worker".to_string(),
        common_fields: Some(FtraceEventCommonFields {
            event_type: 0,
            flags: 0,
            preempt_count: 0,
            pid: 0,
        }),
        sched_switch_format: Some(SchedSwitchFormat {
            prev_comm: "idle".to_string(),
            prev_pid: 0,
            prev_prio: 120,
            prev_state: 0,
            next_comm: "worker".to_string(),
            next_pid: 10,
            next_prio: 110,
        }),
        sched_wakeup_format: None,
        sched_wakeup_new_format: None,
        sched_waking_format: None,
        ..Default::default()
    };
    let wakeup = FtraceEvent {
        timestamp: 100,
        tgid: 1,
        comm: "waker".to_string(),
        common_fields: Some(FtraceEventCommonFields {
            event_type: 0,
            flags: 0,
            preempt_count: 0,
            pid: 20,
        }),
        sched_switch_format: None,
        sched_wakeup_format: Some(SchedWakeupFormat {
            comm: "worker".to_string(),
            pid: 10,
            prio: 110,
            success: 1,
            target_cpu: 0,
        }),
        sched_wakeup_new_format: None,
        sched_waking_format: None,
        ..Default::default()
    };

    let trace = TracePluginResult {
        ftrace_cpu_stats: vec![],
        ftrace_cpu_detail: vec![
            FtraceCpuDetailMsg {
                cpu: 0,
                event: vec![switch],
                overwrite: 0,
            },
            FtraceCpuDetailMsg {
                cpu: 1,
                event: vec![wakeup],
                overwrite: 0,
            },
        ],
        symbols_detail: vec![],
        clocks_detail: vec![],
    };

    let plugin = ProfilerPluginData {
        name: "ftrace-plugin".to_string(),
        status: 0,
        data: trace.encode_to_vec(),
        clock_id: 7,
        tv_sec: 0,
        tv_nsec: 0,
        version: "1.01".to_string(),
        sample_interval: 0,
    };

    let bytes = len_prefixed(plugin);

    let parsed = HtraceParser::default().parse_bytes(&bytes).unwrap();
    assert_non_empty_tables_are_htrace_mvp_tables(&parsed);
    assert_eq!(parsed.tables.thread_state.num_rows(), 2);
}

#[test]
fn parses_memory_plugin_process_and_system_measures() {
    let memory_data = memory::MemoryData {
        processesinfo: vec![memory::ProcessMemoryInfo {
            pid: 42,
            name: "com.demo".to_string(),
            vm_size_kb: 1000,
            vm_rss_kb: 200,
            rss_anon_kb: 120,
            rss_file_kb: 60,
            rss_shmem_kb: 20,
            vm_swap_kb: 5,
            vm_locked_kb: 1,
            vm_hwm_kb: 220,
            oom_score_adj: 100,
            ..Default::default()
        }],
        meminfo: vec![memory::SysMeminfo {
            key: 1,
            value: 8192,
        }],
        ..Default::default()
    };
    let plugin = ProfilerPluginData {
        name: "memory-plugin".to_string(),
        status: 0,
        data: memory_data.encode_to_vec(),
        clock_id: 0,
        tv_sec: 1,
        tv_nsec: 0,
        version: "1.01".to_string(),
        sample_interval: 0,
    };

    let parsed = HtraceParser::default()
        .parse_bytes(&len_prefixed(plugin))
        .unwrap();
    assert_non_empty_tables_are_htrace_mvp_tables(&parsed);
    assert_eq!(parsed.tables.process_measure.num_rows(), 9);
    assert_eq!(parsed.tables.process_measure_filter.num_rows(), 9);
    assert_eq!(parsed.tables.sys_mem_measure.num_rows(), 4);
    assert_eq!(parsed.tables.sys_event_filter.num_rows(), 4);
    assert_eq!(parsed.tables.process.num_rows(), 1);

    let filter_names = parsed
        .tables
        .process_measure_filter
        .column_by_name("name")
        .expect("name column exists")
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("name column is utf8");
    let filter_names = (0..filter_names.len())
        .map(|index| filter_names.value(index))
        .collect::<Vec<_>>();
    assert!(filter_names.contains(&"mem.rss.shmem"));
    assert!(!filter_names.contains(&"mem.rss.schem"));
}

#[test]
fn parses_process_plugin_live_process_samples() {
    let mut bytes = Vec::new();
    for (ts, cpu_time, pss) in [(10, 100, 2048), (20, 140, 3072)] {
        let data = process::ProcessData {
            processesinfo: vec![process::ProcessInfo {
                pid: 42,
                name: "com.demo".to_string(),
                ppid: 1,
                uid: 200100,
                cpuinfo: Some(process::CpuInfo {
                    cpu_usage: 12.5,
                    thread_sum: 8,
                    cpu_time_ms: cpu_time,
                }),
                pssinfo: Some(process::PssInfo { pss_info: pss }),
                diskinfo: Some(process::DiskioInfo {
                    rbytes: 11,
                    wbytes: 22,
                    ..Default::default()
                }),
            }],
        };
        append_segment(
            &mut bytes,
            ProfilerPluginData {
                name: "process-plugin".to_string(),
                status: 0,
                data: data.encode_to_vec(),
                clock_id: 0,
                tv_sec: ts,
                tv_nsec: 0,
                version: "1.01".to_string(),
                sample_interval: 0,
            },
        );
    }

    let parsed = HtraceParser::default().parse_bytes(&bytes).unwrap();
    assert_non_empty_tables_are_htrace_mvp_tables(&parsed);
    assert_eq!(parsed.tables.live_process.num_rows(), 1);
}

#[test]
fn parses_arkts_js_heap_snapshot() {
    let heap_json = r#"{
            "snapshot":{
                "meta":{
                    "node_fields":["type","name","id","self_size","edge_count","trace_node_id","detachedness"],
                    "node_types":[["hidden","object"],"string","number","number","number","number","number"],
                    "edge_fields":["type","name_or_index","to_node"],
                    "edge_types":[["context","element"],"string","node"],
                    "trace_function_info_fields":["function_id","name","script_name","script_id","line","column"],
                    "trace_node_fields":["id","function_info_index","count","size","children"],
                    "sample_fields":["timestamp_us","last_assigned_id"],
                    "location_fields":["object_index","script_id","line","column"]
                },
                "node_count":2,
                "edge_count":1,
                "trace_function_count":1
            },
            "nodes":[1,0,10,64,1,0,0,1,1,20,32,0,0,0],
            "edges":[1,2,7],
            "locations":[],
            "samples":[5,20],
            "strings":["","Object"],
            "trace_function_infos":[1,0,1,7,10,2],
            "trace_tree":[1,0,1,64,[]]
        }"#;

    let mut bytes = Vec::new();
    append_segment(
        &mut bytes,
        ProfilerPluginData {
            name: "arkts-plugin_config".to_string(),
            status: 0,
            data: arkts::ArkTSConfig {
                pid: 42,
                heap_type: arkts::HeapType::Snapshot as i32,
                interval: 1,
                capture_numeric_value: false,
                track_allocations: false,
                enable_cpu_profiler: false,
                cpu_profiler_interval: 0,
            }
            .encode_to_vec(),
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: "1.01".to_string(),
            sample_interval: 0,
        },
    );
    append_segment(
        &mut bytes,
        ProfilerPluginData {
            name: "arkts-plugin".to_string(),
            status: 0,
            data: arkts::ArkTSResult {
                result: format!(
                    r#"{{"params":{{"chunk":{}}}}}"#,
                    serde_json::to_string(heap_json).unwrap()
                )
                .into_bytes(),
            }
            .encode_to_vec(),
            clock_id: 0,
            tv_sec: 1,
            tv_nsec: 0,
            version: "1.01".to_string(),
            sample_interval: 0,
        },
    );
    append_segment(
        &mut bytes,
        ProfilerPluginData {
            name: "arkts-plugin".to_string(),
            status: 0,
            data: arkts::ArkTSResult {
                result: b"{\"id\":1,\"result\":{}}".to_vec(),
            }
            .encode_to_vec(),
            clock_id: 0,
            tv_sec: 2,
            tv_nsec: 0,
            version: "1.01".to_string(),
            sample_interval: 0,
        },
    );

    let parsed = HtraceParser::default().parse_bytes(&bytes).unwrap();
    assert_non_empty_tables_are_htrace_mvp_tables(&parsed);
    assert_eq!(parsed.tables.js_heap_files.num_rows(), 1);
    assert_eq!(parsed.tables.js_heap_nodes.num_rows(), 2);
    assert_eq!(parsed.tables.js_heap_edges.num_rows(), 1);
    assert_eq!(parsed.tables.js_heap_string.num_rows(), 2);
    assert_eq!(parsed.tables.js_heap_trace_node.num_rows(), 1);
}

#[test]
fn parses_arkts_js_cpu_profiler() {
    let profile_json = r#"{
            "nodes":[
                {
                    "id":1,
                    "callFrame":{
                        "functionName":"(root)",
                        "scriptId":"0",
                        "url":"",
                        "lineNumber":0,
                        "columnNumber":0
                    },
                    "hitCount":1,
                    "children":[2]
                },
                {
                    "id":2,
                    "callFrame":{
                        "functionName":"work",
                        "scriptId":"1",
                        "url":"entry.js",
                        "lineNumber":10,
                        "columnNumber":2
                    },
                    "hitCount":3,
                    "children":[]
                }
            ],
            "samples":[1,1,2],
            "timeDeltas":[0,5,7],
            "startTime":100
        }"#;

    let mut bytes = Vec::new();
    append_segment(
        &mut bytes,
        ProfilerPluginData {
            name: "arkts-plugin_config".to_string(),
            status: 0,
            data: arkts::ArkTSConfig {
                pid: 42,
                heap_type: arkts::HeapType::Snapshot as i32,
                interval: 1,
                capture_numeric_value: false,
                track_allocations: false,
                enable_cpu_profiler: true,
                cpu_profiler_interval: 1000,
            }
            .encode_to_vec(),
            clock_id: 0,
            tv_sec: 0,
            tv_nsec: 0,
            version: "1.01".to_string(),
            sample_interval: 0,
        },
    );
    append_segment(
        &mut bytes,
        ProfilerPluginData {
            name: "arkts-plugin".to_string(),
            status: 0,
            data: arkts::ArkTSResult {
                result: format!(r#"{{"id":3,"result":{{"profile":{}}}}}"#, profile_json)
                    .into_bytes(),
            }
            .encode_to_vec(),
            clock_id: 0,
            tv_sec: 1,
            tv_nsec: 0,
            version: "1.01".to_string(),
            sample_interval: 0,
        },
    );

    let parsed = HtraceParser::default().parse_bytes(&bytes).unwrap();
    assert_non_empty_tables_are_htrace_mvp_tables(&parsed);
    assert_eq!(parsed.tables.js_config.num_rows(), 1);
    assert_eq!(parsed.tables.js_cpu_profiler_node.num_rows(), 2);
    assert_eq!(parsed.tables.js_cpu_profiler_sample.num_rows(), 2);
}
