use std::{fs, path::Path};

use arrow_array::{Int32Array, StringArray, UInt32Array, UInt64Array};
use arrow_schema::DataType;
use kat_datasource::DatasetWriteTarget;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use prost::Message;
use serde_json::json;
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
fn formal_import_keeps_native_hook_source_tables_dormant() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("native-hook-and-ftrace.htrace");
    let dataset = root.path().join("dataset");
    fs::write(&source, generated_trace()).expect("generated Hitrace is written");

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |_| Ok(()),
    )
    .expect("formal Hitrace import succeeds");

    let table_names = kat_datasource::inspect_dataset(&dataset)
        .expect("Dataset is inspectable")
        .tables()
        .iter()
        .map(|table| table.name().to_owned())
        .collect::<Vec<_>>();
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

    let clock_domains = batches(&dataset.join("tables/clock_domain.parquet"));
    assert_schema(
        &clock_domains[0],
        &[
            ("clock_domain", DataType::Utf8),
            ("clock_type", DataType::Utf8),
            ("ticks_per_second", DataType::UInt64),
        ],
    );
    assert_eq!(
        clock_domains
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        6
    );
    let clock_domain = clock_domains[0]
        .column_by_name("clock_domain")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let clock_type = clock_domains[0]
        .column_by_name("clock_type")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let ticks = clock_domains[0]
        .column_by_name("ticks_per_second")
        .unwrap()
        .as_any()
        .downcast_ref::<UInt64Array>()
        .unwrap();
    let boottime = (0..clock_domains[0].num_rows())
        .find(|row| clock_domain.value(*row) == "boottime")
        .expect("boottime domain remains present");
    assert_eq!(clock_type.value(boottime), "boottime");
    assert_eq!(ticks.value(boottime), 1_000_000_000);

    let clock_snapshots = batches(&dataset.join("tables/clock_snapshot.parquet"));
    assert_schema(
        &clock_snapshots[0],
        &[
            ("snapshot_id", DataType::UInt64),
            ("clock_domain", DataType::Utf8),
            ("clock_value", DataType::UInt64),
        ],
    );
    assert_eq!(
        clock_snapshots
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        6
    );
    for batch in &clock_snapshots {
        let snapshot_ids = batch
            .column_by_name("snapshot_id")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        assert!((0..batch.num_rows()).all(|row| snapshot_ids.value(row) == 0));
    }

    let switches = batches(&dataset.join("tables/sched_switch.parquet"));
    assert_eq!(switches.len(), 1);
    assert_schema(
        &switches[0],
        &[
            ("clock_domain", DataType::Utf8),
            ("clock_value", DataType::UInt64),
            ("cpu", DataType::UInt32),
            ("cpu_switch_sequence", DataType::UInt64),
            ("previous_thread_id", DataType::Int32),
            ("previous_thread_name", DataType::Utf8),
            ("next_thread_id", DataType::Int32),
            ("next_thread_name", DataType::Utf8),
        ],
    );
    assert_eq!(switches[0].num_rows(), 1);
    assert_eq!(string_value(&switches[0], "clock_domain"), "boottime");
    assert_eq!(u64_value(&switches[0], "clock_value"), 99);
    assert_eq!(u32_value(&switches[0], "cpu"), 0);
    assert_eq!(u64_value(&switches[0], "cpu_switch_sequence"), 0);
    assert_eq!(i32_value(&switches[0], "previous_thread_id"), 0);
    assert_eq!(
        string_value(&switches[0], "previous_thread_name"),
        "swapper"
    );
    assert_eq!(i32_value(&switches[0], "next_thread_id"), 42);
    assert_eq!(string_value(&switches[0], "next_thread_name"), "render");
}

#[tokio::test]
async fn legacy_trace_datasource_still_queries_native_hook_tables() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("native-hook-and-ftrace.htrace");
    fs::write(&source, generated_trace()).expect("generated Hitrace is written");

    let datasource = kat_datasource::TraceDatasource::from_hitrace(&source)
        .expect("legacy TraceDatasource builds");
    assert_eq!(
        datasource
            .query_json("select pid, process_name from native_hook_config")
            .await
            .expect("legacy Native Hook config table is queryable"),
        json!([{"pid": 42, "process_name": "render"}])
    );
    assert_eq!(
        datasource
            .query_json("select tv_sec, tv_nsec, pid, tid, addr, size from native_hook_alloc",)
            .await
            .expect("legacy Native Hook event table is queryable"),
        json!([{
            "tv_sec": 7,
            "tv_nsec": 8,
            "pid": 42,
            "tid": 43,
            "addr": 4096,
            "size": 64,
        }])
    );
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

fn batches(path: &Path) -> Vec<arrow_array::RecordBatch> {
    ParquetRecordBatchReaderBuilder::try_new(fs::File::open(path).expect("Parquet file opens"))
        .expect("Parquet metadata reads")
        .build()
        .expect("Parquet reader builds")
        .collect::<Result<Vec<_>, _>>()
        .expect("Parquet batches read")
}

fn assert_schema(batch: &arrow_array::RecordBatch, expected: &[(&str, DataType)]) {
    let schema = batch.schema();
    let actual = schema
        .fields()
        .iter()
        .map(|field| {
            (
                field.name().as_str(),
                field.data_type().clone(),
                field.is_nullable(),
            )
        })
        .collect::<Vec<_>>();
    let expected = expected
        .iter()
        .map(|(name, data_type)| (*name, data_type.clone(), false))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

fn string_value<'a>(batch: &'a arrow_array::RecordBatch, column: &str) -> &'a str {
    batch
        .column_by_name(column)
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap()
        .value(0)
}

fn u64_value(batch: &arrow_array::RecordBatch, column: &str) -> u64 {
    batch
        .column_by_name(column)
        .unwrap()
        .as_any()
        .downcast_ref::<UInt64Array>()
        .unwrap()
        .value(0)
}

fn u32_value(batch: &arrow_array::RecordBatch, column: &str) -> u32 {
    batch
        .column_by_name(column)
        .unwrap()
        .as_any()
        .downcast_ref::<UInt32Array>()
        .unwrap()
        .value(0)
}

fn i32_value(batch: &arrow_array::RecordBatch, column: &str) -> i32 {
    batch
        .column_by_name(column)
        .unwrap()
        .as_any()
        .downcast_ref::<Int32Array>()
        .unwrap()
        .value(0)
}
