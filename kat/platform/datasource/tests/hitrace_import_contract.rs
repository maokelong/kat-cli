use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use arrow_array::{
    Array, BinaryArray, BooleanArray, Float64Array, Int32Array, Int64Array, StringArray,
    StructArray, UInt32Array, UInt64Array,
};
use kat_datasource::DatasetWriteTarget;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use prost::Message;
use tempfile::tempdir;

const HEADER_SIZE: usize = 1024;
const HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;

#[derive(Clone, PartialEq, Message)]
struct Envelope {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(bytes = "vec", tag = "3")]
    data: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
struct TraceResult {
    #[prost(message, repeated, tag = "1")]
    stats: Vec<Stats>,
    #[prost(message, repeated, tag = "2")]
    details: Vec<Detail>,
    #[prost(message, repeated, tag = "6")]
    clocks: Vec<ClockDetail>,
}

#[derive(Clone, PartialEq, Message)]
struct Stats {
    #[prost(int32, tag = "1")]
    status: i32,
    #[prost(message, repeated, tag = "2")]
    per_cpu: Vec<PerCpuStats>,
    #[prost(string, tag = "3")]
    trace_clock: String,
}

#[derive(Clone, PartialEq, Message)]
struct PerCpuStats {
    #[prost(uint64, tag = "1")]
    cpu: u64,
    #[prost(uint64, tag = "3")]
    overrun: u64,
    #[prost(uint64, tag = "4")]
    commit_overrun: u64,
    #[prost(uint64, tag = "8")]
    dropped_events: u64,
}

#[derive(Clone, PartialEq, Message)]
struct Detail {
    #[prost(uint32, tag = "1")]
    cpu: u32,
    #[prost(message, repeated, tag = "2")]
    events: Vec<Event>,
    #[prost(uint64, tag = "3")]
    overwrite: u64,
}

#[derive(Clone, PartialEq, Message)]
struct Event {
    #[prost(uint64, tag = "1")]
    timestamp: u64,
    #[prost(message, optional, tag = "2417")]
    switch: Option<Switch>,
}

#[derive(Clone, PartialEq, Message)]
struct Switch {
    #[prost(string, tag = "1")]
    previous_name: String,
    #[prost(int32, tag = "2")]
    previous_id: i32,
    #[prost(string, tag = "5")]
    next_name: String,
    #[prost(int32, tag = "6")]
    next_id: i32,
}

#[derive(Clone, PartialEq, Message)]
struct ClockDetail {
    #[prost(int32, tag = "1")]
    id: i32,
    #[prost(message, optional, tag = "2")]
    time: Option<TimeSpec>,
}

#[derive(Clone, Copy, PartialEq, Message)]
struct TimeSpec {
    #[prost(uint32, tag = "1")]
    seconds: u32,
    #[prost(uint32, tag = "2")]
    nanoseconds: u32,
}

#[derive(Clone, PartialEq, Message)]
struct MemoryData {
    #[prost(message, repeated, tag = "1")]
    processesinfo: Vec<ProcessMemoryInfo>,
    #[prost(uint64, tag = "4")]
    zram: u64,
    #[prost(uint64, tag = "9")]
    gpu_limit_size: u64,
    #[prost(uint64, tag = "10")]
    gpu_used_size: u64,
}

#[derive(Clone, PartialEq, Message)]
struct ProcessMemoryInfo {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(string, tag = "2")]
    name: String,
}

#[derive(Clone, PartialEq, Message)]
struct CpuConfig {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(bool, tag = "2")]
    report_process_info: bool,
}

#[derive(Clone, PartialEq, Message)]
struct CpuData {
    #[prost(int64, tag = "3")]
    process_num: i64,
    #[prost(double, tag = "4")]
    user_load: f64,
}

#[derive(Clone, PartialEq, Message)]
struct MemoryConfig {
    #[prost(bool, tag = "1")]
    report_process_tree: bool,
    #[prost(int32, repeated, tag = "3")]
    sys_meminfo_counters: Vec<i32>,
}

#[derive(Clone, PartialEq, Message)]
struct ProcessConfig {
    #[prost(bool, tag = "1")]
    report_process_tree: bool,
}

#[derive(Clone, PartialEq, Message)]
struct ProcessData {
    #[prost(message, repeated, tag = "1")]
    processesinfo: Vec<ProcessInfo>,
}

#[derive(Clone, PartialEq, Message)]
struct ProcessInfo {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(string, tag = "2")]
    name: String,
    #[prost(message, optional, tag = "5")]
    cpuinfo: Option<CpuInfo>,
}

#[derive(Clone, PartialEq, Message)]
struct CpuInfo {
    #[prost(double, tag = "1")]
    cpu_usage: f64,
    #[prost(int32, tag = "2")]
    thread_sum: i32,
}

#[derive(Clone, PartialEq, Message)]
struct DiskioConfig {
    #[prost(int32, tag = "2")]
    report_io_stats: i32,
}

#[derive(Clone, PartialEq, Message)]
struct DiskioData {
    #[prost(message, optional, tag = "7")]
    stats_data: Option<DiskioStatsData>,
}

#[derive(Clone, PartialEq, Message)]
struct DiskioStatsData {
    #[prost(message, repeated, tag = "1")]
    cpuinfo: Vec<DiskioCpuStats>,
}

#[derive(Clone, PartialEq, Message)]
struct DiskioCpuStats {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(double, tag = "2")]
    cpu_user: f64,
}

#[derive(Clone, PartialEq, Message)]
struct NetworkConfig {
    #[prost(int32, repeated, tag = "1")]
    pid: Vec<i32>,
    #[prost(string, tag = "2")]
    test_file: String,
}

#[derive(Clone, PartialEq, Message)]
struct NetworkDatas {
    #[prost(message, repeated, tag = "1")]
    networkinfo: Vec<NetworkData>,
}

#[derive(Clone, PartialEq, Message)]
struct NetworkData {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(uint64, tag = "4")]
    tx_bytes: u64,
}

#[derive(Clone, PartialEq, Message)]
struct GpuConfig {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(bool, tag = "2")]
    report_gpu_info: bool,
}

#[derive(Clone, PartialEq, Message)]
struct GpuData {
    #[prost(uint64, tag = "1")]
    boottime: u64,
    #[prost(uint64, tag = "2")]
    gpu_utilisation: u64,
    #[prost(message, repeated, tag = "3")]
    gpu_data_array: Vec<GpuDataExt>,
}

#[derive(Clone, PartialEq, Message)]
struct GpuDataExt {
    #[prost(uint64, tag = "1")]
    boottime: u64,
    #[prost(uint64, tag = "2")]
    gpu_utilisation: u64,
}

#[derive(Clone, PartialEq, Message)]
struct NativeHookConfigFixture {
    #[prost(int32, tag = "1")]
    pid: i32,
}

#[derive(Clone, PartialEq, Message)]
struct NativeHookBatchFixture {
    #[prost(message, repeated, tag = "1")]
    events: Vec<NativeHookEventFixture>,
}

#[derive(Clone, PartialEq, Message)]
struct NativeHookEventFixture {
    #[prost(uint64, tag = "1")]
    tv_sec: u64,
    #[prost(uint64, tag = "2")]
    tv_nsec: u64,
    #[prost(oneof = "native_hook_event_fixture::Event", tags = "3, 12, 13, 14")]
    event: Option<native_hook_event_fixture::Event>,
}

mod native_hook_event_fixture {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub(super) enum Event {
        #[prost(message, tag = "3")]
        Alloc(super::NativeHookAllocFixture),
        #[prost(message, tag = "12")]
        SymbolTable(super::NativeHookSymbolTableFixture),
        #[prost(message, tag = "13")]
        FrameMap(super::NativeHookFrameMapFixture),
        #[prost(message, tag = "14")]
        StackMap(super::NativeHookStackMapFixture),
    }
}

#[derive(Clone, PartialEq, Message)]
struct NativeHookAllocFixture {
    #[prost(int32, tag = "1")]
    pid: i32,
    #[prost(int32, tag = "2")]
    tid: i32,
    #[prost(uint64, tag = "3")]
    addr: u64,
    #[prost(uint64, tag = "4")]
    size: u64,
    #[prost(message, repeated, tag = "5")]
    frame_info: Vec<NativeHookFrameFixture>,
}

#[derive(Clone, PartialEq, Message)]
struct NativeHookFrameFixture {
    #[prost(uint64, tag = "1")]
    ip: u64,
    #[prost(string, tag = "3")]
    symbol_name: String,
    #[prost(string, tag = "4")]
    file_path: String,
}

#[derive(Clone, PartialEq, Message)]
struct NativeHookStackMapFixture {
    #[prost(uint32, tag = "1")]
    id: u32,
    #[prost(uint64, repeated, tag = "2")]
    frame_map_id: Vec<u64>,
    #[prost(uint64, repeated, tag = "3")]
    ip: Vec<u64>,
    #[prost(int32, tag = "4")]
    pid: i32,
}

#[derive(Clone, PartialEq, Message)]
struct NativeHookSymbolTableFixture {
    #[prost(uint32, tag = "1")]
    file_path_id: u32,
    #[prost(bytes = "vec", tag = "5")]
    sym_table: Vec<u8>,
    #[prost(bytes = "vec", tag = "6")]
    str_table: Vec<u8>,
    #[prost(int32, tag = "7")]
    pid: i32,
}

#[derive(Clone, PartialEq, Message)]
struct NativeHookFrameMapFixture {
    #[prost(uint32, tag = "1")]
    id: u32,
    #[prost(message, optional, tag = "2")]
    frame: Option<NativeHookFrameFixture>,
    #[prost(int32, tag = "3")]
    pid: i32,
}

fn cpu_stats(cpu: u64) -> PerCpuStats {
    PerCpuStats {
        cpu,
        ..Default::default()
    }
}

fn stats(status: i32, clock: &str, cpus: &[u64]) -> Stats {
    Stats {
        status,
        per_cpu: cpus.iter().copied().map(cpu_stats).collect(),
        trace_clock: clock.to_owned(),
    }
}

fn switch(timestamp: u64, previous_id: i32, next_id: i32) -> Event {
    Event {
        timestamp,
        switch: Some(Switch {
            previous_name: format!("thread-{previous_id}"),
            previous_id,
            next_name: format!("thread-{next_id}"),
            next_id,
        }),
    }
}

fn detail(cpu: u32, events: Vec<Event>) -> Detail {
    Detail {
        cpu,
        events,
        overwrite: 0,
    }
}

fn complete_result(clock: &str, details: Vec<Detail>) -> TraceResult {
    let cpus = details
        .iter()
        .map(|detail| u64::from(detail.cpu))
        .collect::<Vec<_>>();
    TraceResult {
        stats: vec![stats(0, clock, &cpus), stats(1, clock, &cpus)],
        details,
        clocks: Vec::new(),
    }
}

fn fixture(result: TraceResult) -> Vec<u8> {
    fixture_results([result])
}

fn fixture_results(results: impl IntoIterator<Item = TraceResult>) -> Vec<u8> {
    let envelopes = results
        .into_iter()
        .map(|result| {
            Envelope {
                name: "ftrace-plugin".to_owned(),
                data: result.encode_to_vec(),
            }
            .encode_to_vec()
        })
        .collect::<Vec<_>>();
    profiler_fixture(envelopes)
}

fn profiler_fixture(envelopes: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    let envelopes = envelopes.into_iter().collect::<Vec<_>>();
    let body_length = envelopes
        .iter()
        .map(|envelope| 4 + envelope.len())
        .sum::<usize>();
    let mut bytes = vec![0; HEADER_SIZE];
    bytes[0..8].copy_from_slice(&HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((HEADER_SIZE + body_length) as u64).to_le_bytes());
    for (offset, value) in [60, 68, 76, 84, 92, 100].into_iter().zip(1_u64..=6) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    for envelope in envelopes {
        bytes.extend_from_slice(&(envelope.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&envelope);
    }
    bytes
}

fn import_fixed_result<C, D>(plugin_name: &str, config: &C, data: &D) -> tempfile::TempDir
where
    C: Message,
    D: Message,
{
    let root = tempdir().expect("tempdir");
    let source = root.path().join(format!("{plugin_name}.htrace"));
    let dataset = root.path().join("dataset");
    let envelopes = [
        Envelope {
            name: format!("{plugin_name}_config"),
            data: config.encode_to_vec(),
        }
        .encode_to_vec(),
        Envelope {
            name: plugin_name.to_owned(),
            data: data.encode_to_vec(),
        }
        .encode_to_vec(),
    ];
    fs::write(&source, profiler_fixture(envelopes)).expect("Hitrace fixture is written");
    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |_| Ok(()),
    )
    .unwrap_or_else(|error| panic!("{plugin_name} fixed-result payload imports: {error:#}"));
    root
}

fn import_native_hook(data: &NativeHookBatchFixture) -> tempfile::TempDir {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("nativehook.htrace");
    let dataset = root.path().join("dataset");
    let envelopes = [
        Envelope {
            name: "nativehook_config".to_owned(),
            data: NativeHookConfigFixture { pid: 4242 }.encode_to_vec(),
        }
        .encode_to_vec(),
        Envelope {
            name: "nativehook".to_owned(),
            data: data.encode_to_vec(),
        }
        .encode_to_vec(),
    ];
    fs::write(&source, profiler_fixture(envelopes)).expect("Hitrace fixture is written");
    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |_| Ok(()),
    )
    .unwrap_or_else(|error| panic!("nativehook payload imports: {error:#}"));
    root
}

fn single_row_batch(root: &tempfile::TempDir, table_name: &str) -> arrow_array::RecordBatch {
    let mut table_batches = batches(
        &root
            .path()
            .join(format!("dataset/tables/{table_name}.parquet")),
    );
    assert_eq!(
        table_batches
            .iter()
            .map(arrow_array::RecordBatch::num_rows)
            .sum::<usize>(),
        1,
        "{table_name} should contain one row"
    );
    assert_eq!(
        table_batches.len(),
        1,
        "{table_name} should contain one batch"
    );
    table_batches.remove(0)
}

fn write_fixture(path: &Path, result: TraceResult) {
    fs::write(path, fixture(result)).expect("Hitrace fixture is written");
}

#[test]
fn import_decodes_cpu_config_and_data_into_relational_tables() {
    let root = import_fixed_result(
        "cpu-plugin",
        &CpuConfig {
            pid: 1234,
            report_process_info: true,
        },
        &CpuData {
            process_num: 37,
            user_load: 12.5,
        },
    );

    let config = single_row_batch(&root, "cpu_config");
    assert_eq!(
        config
            .column_by_name("pid")
            .expect("pid")
            .as_any()
            .downcast_ref::<Int32Array>()
            .expect("pid is Int32")
            .value(0),
        1234
    );
    assert!(
        config
            .column_by_name("report_process_info")
            .expect("report_process_info")
            .as_any()
            .downcast_ref::<BooleanArray>()
            .expect("report_process_info is Boolean")
            .value(0)
    );

    let data = single_row_batch(&root, "cpu_data");
    assert_eq!(
        data.column_by_name("process_num")
            .expect("process_num")
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("process_num is Int64")
            .value(0),
        37
    );
    assert_eq!(
        data.column_by_name("user_load")
            .expect("user_load")
            .as_any()
            .downcast_ref::<Float64Array>()
            .expect("user_load is Float64")
            .value(0),
        12.5
    );
}

#[test]
fn import_decodes_memory_config_and_data_into_relational_tables() {
    let payload = MemoryData {
        processesinfo: vec![ProcessMemoryInfo {
            pid: 42,
            name: "render-service".to_owned(),
        }],
        zram: 4096,
        gpu_limit_size: 8192,
        gpu_used_size: 2048,
    };
    let root = import_fixed_result(
        "memory-plugin",
        &MemoryConfig {
            report_process_tree: true,
            sys_meminfo_counters: vec![1, 2],
        },
        &payload,
    );
    let dataset = root.path().join("dataset");
    let inspection = kat_datasource::inspect_dataset(&dataset).expect("Dataset inspects");
    for table in ["memory_config", "memory_data", "memory_data_processesinfo"] {
        assert!(
            inspection
                .tables()
                .iter()
                .any(|candidate| candidate.name() == table),
            "{table} should be materialized"
        );
    }

    let config_batches = batches(&dataset.join("tables/memory_config.parquet"));
    assert!(
        config_batches[0]
            .column_by_name("report_process_tree")
            .expect("report_process_tree")
            .as_any()
            .downcast_ref::<BooleanArray>()
            .expect("report_process_tree is Boolean")
            .value(0)
    );
    let counter_batches =
        batches(&dataset.join("tables/memory_config_sys_meminfo_counters.parquet"));
    let counter_values = counter_batches
        .iter()
        .flat_map(|batch| {
            let values = batch
                .column_by_name("value")
                .expect("counter value")
                .as_any()
                .downcast_ref::<Int32Array>()
                .expect("counter value is Int32")
                .clone();
            let names = batch
                .column_by_name("value_name")
                .expect("counter value_name")
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("counter value_name is Utf8")
                .clone();
            (0..batch.num_rows())
                .map(move |index| (values.value(index), names.value(index).to_owned()))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        counter_values,
        [
            (1, "PMEM_MEM_TOTAL".to_owned()),
            (2, "PMEM_MEM_FREE".to_owned()),
        ]
    );

    let root_batches = batches(&dataset.join("tables/memory_data.parquet"));
    assert_eq!(
        root_batches
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1
    );
    let root_batch = &root_batches[0];
    let root_row_index = root_batch
        .column_by_name("row_index")
        .expect("root row_index")
        .as_any()
        .downcast_ref::<UInt64Array>()
        .expect("root row_index is UInt64")
        .value(0);
    assert_eq!(
        root_batch
            .column_by_name("zram")
            .expect("zram")
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("zram is UInt64")
            .value(0),
        4096
    );

    let process_batches = batches(&dataset.join("tables/memory_data_processesinfo.parquet"));
    assert_eq!(
        process_batches
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1
    );
    let process_batch = &process_batches[0];
    assert_eq!(
        process_batch
            .column_by_name("pid")
            .expect("pid")
            .as_any()
            .downcast_ref::<Int32Array>()
            .expect("pid is Int32")
            .value(0),
        42
    );
    assert_eq!(
        process_batch
            .column_by_name("name")
            .expect("name")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("name is Utf8")
            .value(0),
        "render-service"
    );
    let source_index = process_batch
        .column_by_name("source_index")
        .expect("source_index")
        .as_any()
        .downcast_ref::<UInt64Array>()
        .expect("source_index is UInt64")
        .value(0);
    let parent_index = process_batch
        .column_by_name("parent_index")
        .expect("parent_index")
        .as_any()
        .downcast_ref::<UInt64Array>()
        .expect("parent_index is UInt64")
        .value(0);
    assert_eq!(source_index, 0);
    assert_eq!(parent_index, root_row_index);
}

#[test]
fn import_decodes_process_config_and_data_into_relational_tables() {
    let root = import_fixed_result(
        "process-plugin",
        &ProcessConfig {
            report_process_tree: true,
        },
        &ProcessData {
            processesinfo: vec![ProcessInfo {
                pid: 2468,
                name: "system-service".to_owned(),
                cpuinfo: Some(CpuInfo {
                    cpu_usage: 37.5,
                    thread_sum: 12,
                }),
            }],
        },
    );

    let config = single_row_batch(&root, "process_config");
    assert!(
        config
            .column_by_name("report_process_tree")
            .expect("report_process_tree")
            .as_any()
            .downcast_ref::<BooleanArray>()
            .expect("report_process_tree is Boolean")
            .value(0)
    );

    let data = single_row_batch(&root, "process_data_processesinfo");
    assert_eq!(
        data.column_by_name("pid")
            .expect("pid")
            .as_any()
            .downcast_ref::<Int32Array>()
            .expect("pid is Int32")
            .value(0),
        2468
    );
    assert_eq!(
        data.column_by_name("name")
            .expect("name")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("name is Utf8")
            .value(0),
        "system-service"
    );
    let data_schema = data.schema();
    let cpuinfo_field = data_schema
        .field_with_name("cpuinfo")
        .expect("cpuinfo schema field");
    assert!(cpuinfo_field.is_nullable());
    let cpuinfo = data
        .column_by_name("cpuinfo")
        .expect("cpuinfo")
        .as_any()
        .downcast_ref::<StructArray>()
        .expect("cpuinfo is Struct");
    assert!(!cpuinfo.is_null(0));
    assert_eq!(
        cpuinfo
            .column_by_name("cpu_usage")
            .expect("cpu_usage")
            .as_any()
            .downcast_ref::<Float64Array>()
            .expect("cpu_usage is Float64")
            .value(0),
        37.5
    );
}

#[test]
fn import_decodes_diskio_config_and_camel_case_data_fields() {
    let payload = DiskioData {
        stats_data: Some(DiskioStatsData {
            cpuinfo: vec![DiskioCpuStats {
                name: "cpu0".to_owned(),
                cpu_user: 12.5,
            }],
        }),
    };
    let root = import_fixed_result(
        "diskio-plugin",
        &DiskioConfig { report_io_stats: 2 },
        &payload,
    );
    let dataset = root.path().join("dataset");

    let config_batches = batches(&dataset.join("tables/diskio_config.parquet"));
    assert_eq!(
        config_batches[0]
            .column_by_name("report_io_stats")
            .expect("report_io_stats")
            .as_any()
            .downcast_ref::<Int32Array>()
            .expect("report_io_stats is Int32")
            .value(0),
        2
    );
    assert_eq!(
        config_batches[0]
            .column_by_name("report_io_stats_name")
            .expect("report_io_stats_name")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("report_io_stats_name is Utf8")
            .value(0),
        "IO_REPORT_EX"
    );

    let table = dataset.join("tables/diskio_data_stats_data_cpuinfo.parquet");
    let batches = batches(&table);
    assert_eq!(
        batches
            .iter()
            .map(arrow_array::RecordBatch::num_rows)
            .sum::<usize>(),
        1
    );
    let batch = &batches[0];
    assert_eq!(
        batch
            .column_by_name("name")
            .expect("name")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("name is Utf8")
            .value(0),
        "cpu0"
    );
    assert_eq!(
        batch
            .column_by_name("cpu_user")
            .expect("cpu_user")
            .as_any()
            .downcast_ref::<Float64Array>()
            .expect("cpu_user is Float64")
            .value(0),
        12.5
    );
}

#[test]
fn import_decodes_network_config_and_data_into_relational_tables() {
    let root = import_fixed_result(
        "network-plugin",
        &NetworkConfig {
            pid: vec![3141],
            test_file: "/proc/net/dev".to_owned(),
        },
        &NetworkDatas {
            networkinfo: vec![NetworkData {
                pid: 3141,
                tx_bytes: 65_536,
            }],
        },
    );

    let config = single_row_batch(&root, "network_config");
    assert_eq!(
        config
            .column_by_name("test_file")
            .expect("test_file")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("test_file is Utf8")
            .value(0),
        "/proc/net/dev"
    );
    let pid = single_row_batch(&root, "network_config_pid");
    assert_eq!(
        pid.column_by_name("value")
            .expect("pid value")
            .as_any()
            .downcast_ref::<Int32Array>()
            .expect("pid value is Int32")
            .value(0),
        3141
    );

    let data = single_row_batch(&root, "network_datas_networkinfo");
    assert_eq!(
        data.column_by_name("pid")
            .expect("pid")
            .as_any()
            .downcast_ref::<Int32Array>()
            .expect("pid is Int32")
            .value(0),
        3141
    );
    assert_eq!(
        data.column_by_name("tx_bytes")
            .expect("tx_bytes")
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("tx_bytes is UInt64")
            .value(0),
        65_536
    );
}

#[test]
fn import_decodes_gpu_config_and_data_into_relational_tables() {
    let root = import_fixed_result(
        "gpu-plugin",
        &GpuConfig {
            pid: 2718,
            report_gpu_info: true,
        },
        &GpuData {
            boottime: 1_000,
            gpu_utilisation: 45,
            gpu_data_array: vec![GpuDataExt {
                boottime: 1_001,
                gpu_utilisation: 55,
            }],
        },
    );

    let config = single_row_batch(&root, "gpu_config");
    assert_eq!(
        config
            .column_by_name("pid")
            .expect("pid")
            .as_any()
            .downcast_ref::<Int32Array>()
            .expect("pid is Int32")
            .value(0),
        2718
    );
    assert!(
        config
            .column_by_name("report_gpu_info")
            .expect("report_gpu_info")
            .as_any()
            .downcast_ref::<BooleanArray>()
            .expect("report_gpu_info is Boolean")
            .value(0)
    );

    let data = single_row_batch(&root, "gpu_data");
    assert_eq!(
        data.column_by_name("gpu_utilisation")
            .expect("gpu_utilisation")
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("gpu_utilisation is UInt64")
            .value(0),
        45
    );
    let child = single_row_batch(&root, "gpu_data_gpu_data_array");
    assert_eq!(
        child
            .column_by_name("gpu_utilisation")
            .expect("gpu_utilisation")
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("gpu_utilisation is UInt64")
            .value(0),
        55
    );
}

#[test]
fn import_preserves_native_hook_structural_rule_contracts_across_flushes() {
    use native_hook_event_fixture::Event;

    let root = import_native_hook(&NativeHookBatchFixture {
        events: vec![
            NativeHookEventFixture {
                tv_sec: 10,
                tv_nsec: 1,
                event: Some(Event::Alloc(NativeHookAllocFixture {
                    pid: 4242,
                    tid: 7,
                    addr: 0x1000,
                    size: 128,
                    frame_info: vec![NativeHookFrameFixture {
                        ip: 0x2000,
                        symbol_name: "allocate".to_owned(),
                        file_path: "/system/lib/libsample.so".to_owned(),
                    }],
                })),
            },
            NativeHookEventFixture {
                tv_sec: 11,
                tv_nsec: 2,
                event: Some(Event::StackMap(NativeHookStackMapFixture {
                    id: 1,
                    frame_map_id: Vec::new(),
                    ip: (0..=65_536).map(|value| 0x3000 + value).collect(),
                    pid: 4242,
                })),
            },
            NativeHookEventFixture {
                tv_sec: 12,
                tv_nsec: 3,
                event: Some(Event::SymbolTable(NativeHookSymbolTableFixture {
                    file_path_id: 9,
                    sym_table: vec![0x00, 0xff, 0x41],
                    str_table: vec![0x10, 0x20],
                    pid: 4242,
                })),
            },
            NativeHookEventFixture {
                tv_sec: 13,
                tv_nsec: 4,
                event: Some(Event::FrameMap(NativeHookFrameMapFixture {
                    id: 2,
                    frame: Some(NativeHookFrameFixture {
                        ip: 0x4000,
                        symbol_name: "render".to_owned(),
                        file_path: "/system/lib/librender.so".to_owned(),
                    }),
                    pid: 4242,
                })),
            },
            NativeHookEventFixture {
                tv_sec: 14,
                tv_nsec: 5,
                event: Some(Event::FrameMap(NativeHookFrameMapFixture {
                    id: 3,
                    frame: None,
                    pid: 4242,
                })),
            },
        ],
    });
    let dataset = root.path().join("dataset");

    let event_names = batches(&dataset.join("tables/batch_native_hook_data_events.parquet"))
        .into_iter()
        .flat_map(|batch| {
            let values = batch
                .column_by_name("event")
                .expect("event variant")
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("event variant is Utf8")
                .clone();
            (0..batch.num_rows()).map(move |index| values.value(index).to_owned())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        event_names,
        [
            "alloc_event",
            "stack_map",
            "symbol_tab",
            "frame_map",
            "frame_map"
        ]
    );

    let alloc = single_row_batch(&root, "batch_native_hook_data_events_alloc_event");
    assert_eq!(
        alloc
            .column_by_name("parent_index")
            .expect("alloc parent_index")
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("alloc parent_index is UInt64")
            .value(0),
        0
    );
    let frame = single_row_batch(
        &root,
        "batch_native_hook_data_events_alloc_event_frame_info",
    );
    assert_eq!(
        frame
            .column_by_name("parent_index")
            .expect("frame parent_index")
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("frame parent_index is UInt64")
            .value(0),
        0
    );

    let stack_map = single_row_batch(&root, "batch_native_hook_data_events_stack_map");
    assert_eq!(
        stack_map
            .column_by_name("parent_index")
            .expect("stack_map parent_index")
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("stack_map parent_index is UInt64")
            .value(0),
        1
    );
    let mut expected_row_index = 0_u64;
    for batch in batches(&dataset.join("tables/batch_native_hook_data_events_stack_map_ip.parquet"))
    {
        let values = batch
            .column_by_name("value")
            .expect("ip value")
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("ip value is UInt64");
        let parents = batch
            .column_by_name("parent_index")
            .expect("ip parent_index")
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("ip parent_index is UInt64");
        let rows = batch
            .column_by_name("row_index")
            .expect("ip row_index")
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("ip row_index is UInt64");
        for index in 0..batch.num_rows() {
            assert_eq!(parents.value(index), 0);
            assert_eq!(rows.value(index), expected_row_index);
            assert_eq!(values.value(index), 0x3000 + expected_row_index);
            expected_row_index += 1;
        }
    }
    assert_eq!(expected_row_index, 65_537);

    let symbol = single_row_batch(&root, "batch_native_hook_data_events_symbol_tab");
    assert_eq!(
        symbol
            .column_by_name("sym_table")
            .expect("sym_table")
            .as_any()
            .downcast_ref::<BinaryArray>()
            .expect("sym_table is Binary")
            .value(0),
        [0x00, 0xff, 0x41]
    );

    let frame_maps =
        batches(&dataset.join("tables/batch_native_hook_data_events_frame_map.parquet"));
    assert_eq!(
        frame_maps
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        2
    );
    let frame_maps = &frame_maps[0];
    let frame_struct = frame_maps
        .column_by_name("frame")
        .expect("frame Struct")
        .as_any()
        .downcast_ref::<StructArray>()
        .expect("frame is Struct");
    assert!(!frame_struct.is_null(0));
    assert!(frame_struct.is_null(1));
}

#[test]
fn import_publishes_long_term_clock_and_switch_facts_in_source_order() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("capture.htrace");
    let dataset = root.path().join("dataset");
    let mut result = complete_result(
        "local",
        vec![
            detail(0, vec![switch(10, 0, 1), switch(10, 1, 2)]),
            detail(1, vec![switch(7, 0, 9)]),
        ],
    );
    result.clocks = vec![
        ClockDetail {
            id: 1,
            time: Some(TimeSpec {
                seconds: 12,
                nanoseconds: 34,
            }),
        },
        ClockDetail {
            id: 2,
            time: Some(TimeSpec {
                seconds: 56,
                nanoseconds: 78,
            }),
        },
    ];
    write_fixture(&source, result);

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |_| Ok(()),
    )
    .expect("complete capture imports");
    let tables = kat_datasource::inspect_dataset(&dataset)
        .expect("Dataset is inspectable")
        .tables()
        .iter()
        .map(|table| table.name().to_owned())
        .collect::<Vec<_>>();
    for fact in ["clock_domain", "clock_snapshot", "sched_switch"] {
        assert!(
            tables.iter().any(|table| table == fact),
            "{fact} is preserved"
        );
    }
    for relational in [
        "trace_plugin_result",
        "trace_plugin_result_ftrace_cpu_detail",
        "trace_plugin_result_ftrace_cpu_detail_event",
    ] {
        assert!(
            tables.iter().any(|table| table == relational),
            "{relational} is added"
        );
    }

    let mut rows = Vec::new();
    for batch in batches(&dataset.join("tables/sched_switch.parquet")) {
        let domains = batch
            .column_by_name("clock_domain")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let clocks = batch
            .column_by_name("clock_value")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let cpus = batch
            .column_by_name("cpu")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt32Array>()
            .unwrap();
        let sequences = batch
            .column_by_name("cpu_switch_sequence")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let previous = batch
            .column_by_name("previous_thread_id")
            .unwrap()
            .as_any()
            .downcast_ref::<Int32Array>()
            .unwrap();
        let next = batch
            .column_by_name("next_thread_id")
            .unwrap()
            .as_any()
            .downcast_ref::<Int32Array>()
            .unwrap();
        for row in 0..batch.num_rows() {
            rows.push((
                domains.value(row).to_owned(),
                clocks.value(row),
                cpus.value(row),
                sequences.value(row),
                previous.value(row),
                next.value(row),
            ));
        }
    }
    assert_eq!(
        rows,
        [
            ("ftrace_local_cpu_0".to_owned(), 10, 0, 0, 0, 1),
            ("ftrace_local_cpu_0".to_owned(), 10, 0, 1, 1, 2),
            ("ftrace_local_cpu_1".to_owned(), 7, 1, 0, 0, 9),
        ]
    );
    assert_eq!(
        batches(&dataset.join("tables/clock_snapshot.parquet"))
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        8
    );
}

#[test]
fn import_batches_switches_without_deriving_sequence_from_parquet_order() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("capture.htrace");
    let dataset = root.path().join("dataset");
    let mut events = Vec::new();
    let mut previous = 0;
    for sequence in 0..=8192 {
        let next = sequence + 1;
        events.push(switch(sequence as u64, previous, next));
        previous = next;
    }
    write_fixture(&source, complete_result("boot", vec![detail(3, events)]));

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |_| Ok(()),
    )
    .expect("capture imports");
    let batches = batches(&dataset.join("tables/sched_switch.parquet"));
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        8193
    );
    let last = batches.last().expect("at least one switch batch");
    let sequences = last
        .column_by_name("cpu_switch_sequence")
        .unwrap()
        .as_any()
        .downcast_ref::<UInt64Array>()
        .unwrap();
    assert_eq!(sequences.value(sequences.len() - 1), 8192);
}

#[test]
fn import_batches_clock_snapshots_without_changing_source_order() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("clock-snapshots.htrace");
    let dataset = root.path().join("dataset");
    let results = (0_u32..=8192).map(|value| TraceResult {
        stats: Vec::new(),
        details: Vec::new(),
        clocks: vec![ClockDetail {
            id: 1,
            time: Some(TimeSpec {
                seconds: value,
                nanoseconds: value,
            }),
        }],
    });
    fs::write(&source, fixture_results(results)).expect("trace is written");

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |_| Ok(()),
    )
    .expect("clock snapshots import");

    let batches = batches(&dataset.join("tables/clock_snapshot.parquet"));
    assert!(batches.len() >= 2, "clock snapshots cross a batch boundary");
    let rows = batches
        .iter()
        .flat_map(|batch| {
            let ids = batch
                .column_by_name("snapshot_id")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap();
            let domains = batch
                .column_by_name("clock_domain")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let values = batch
                .column_by_name("clock_value")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap();
            (0..batch.num_rows())
                .map(|row| {
                    (
                        ids.value(row),
                        domains.value(row).to_owned(),
                        values.value(row),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 6 + 8193);
    assert!(
        rows[..6]
            .iter()
            .all(|(snapshot_id, _, _)| *snapshot_id == 0)
    );
    assert_eq!(rows[6], (1, "boottime".to_owned(), 0));
    assert_eq!(
        rows.last(),
        Some(&(8193, "boottime".to_owned(), 8_192_000_008_192))
    );
}

#[test]
fn clock_and_thread_continuity_damage_leave_no_published_overwrite_target() {
    for (events, expected) in [
        (
            vec![switch(2, 0, 1), switch(1, 1, 2)],
            "clock went backwards",
        ),
        (
            vec![switch(1, 0, 1), switch(2, 7, 2)],
            "thread continuity is broken",
        ),
    ] {
        let root = tempdir().expect("tempdir");
        let source = root.path().join("capture.htrace");
        let dataset = root.path().join("dataset");
        fs::create_dir(&dataset).expect("target exists");
        fs::write(dataset.join("sentinel"), "unchanged").expect("sentinel exists");
        write_fixture(&source, complete_result("boot", vec![detail(0, events)]));

        let error = kat_datasource::import_hitrace(
            &source,
            DatasetWriteTarget::permanently_replace_all_contents(&dataset),
            |_| Ok(()),
        )
        .expect_err("damaged capture is rejected");
        let message = format!("{error:?}");
        assert!(message.contains(expected), "{message}");
        assert!(!dataset.join("sentinel").exists());
        assert!(!dataset.join(".kat-dataset").exists());
    }
}

#[test]
fn protected_path_inside_overwrite_target_fails_before_any_mutation() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("capture.htrace");
    let dataset = root.path().join("dataset");
    let protected = dataset.join("logs/operation.log");
    write_fixture(
        &source,
        complete_result("boot", vec![detail(0, vec![switch(1, 0, 1)])]),
    );
    fs::create_dir_all(protected.parent().unwrap()).expect("target exists");
    fs::write(dataset.join(".kat-dataset"), "").expect("marker exists");
    fs::write(dataset.join("sentinel"), "unchanged").expect("sentinel exists");
    fs::write(&protected, "operation evidence").expect("protected file exists");

    let error = kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::permanently_replace_all_contents(&dataset).protect_path(&protected),
        |_| Ok(()),
    )
    .expect_err("overlapping protected path is rejected");

    assert!(error.to_string().contains("protected path"), "{error:?}");
    assert!(dataset.join(".kat-dataset").is_file());
    assert_eq!(
        fs::read_to_string(dataset.join("sentinel")).expect("sentinel remains"),
        "unchanged"
    );
    assert_eq!(
        fs::read_to_string(&protected).expect("protected evidence remains"),
        "operation evidence"
    );
}

#[test]
fn protected_sibling_does_not_block_authorized_overwrite() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("capture.htrace");
    let dataset = root.path().join("dataset");
    let protected = root.path().join("operation.log");
    write_fixture(
        &source,
        complete_result("boot", vec![detail(0, vec![switch(1, 0, 1)])]),
    );
    fs::create_dir(&dataset).expect("target exists");
    fs::write(dataset.join("sentinel"), "replace me").expect("sentinel exists");
    fs::write(&protected, "operation evidence").expect("protected file exists");

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::permanently_replace_all_contents(&dataset).protect_path(&protected),
        |_| Ok(()),
    )
    .expect("sibling protected path is outside the target");

    assert!(!dataset.join("sentinel").exists());
    assert_eq!(
        fs::read_to_string(&protected).expect("protected evidence remains"),
        "operation evidence"
    );
}

#[cfg(unix)]
#[test]
fn protected_path_check_resolves_symlinked_overwrite_target() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("capture.htrace");
    let real_dataset = root.path().join("real-dataset");
    let linked_dataset = root.path().join("linked-dataset");
    let protected = real_dataset.join("logs/operation.log");
    write_fixture(
        &source,
        complete_result("boot", vec![detail(0, vec![switch(1, 0, 1)])]),
    );
    fs::create_dir_all(protected.parent().unwrap()).expect("target exists");
    fs::write(real_dataset.join(".kat-dataset"), "").expect("marker exists");
    fs::write(real_dataset.join("sentinel"), "unchanged").expect("sentinel exists");
    fs::write(&protected, "operation evidence").expect("protected file exists");
    std::os::unix::fs::symlink(&real_dataset, &linked_dataset).expect("target symlink exists");

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::permanently_replace_all_contents(&linked_dataset)
            .protect_path(&protected),
        |_| Ok(()),
    )
    .expect_err("canonical target contains the protected path");

    assert!(real_dataset.join(".kat-dataset").is_file());
    assert_eq!(
        fs::read_to_string(real_dataset.join("sentinel")).expect("sentinel remains"),
        "unchanged"
    );
}

#[test]
fn every_loss_evidence_rejects_the_complete_import() {
    for counter in ["overrun", "commit_overrun", "dropped_events", "overwrite"] {
        let root = tempdir().expect("tempdir");
        let source = root.path().join("capture.htrace");
        let dataset = root.path().join("dataset");
        let mut result = complete_result("global", vec![detail(0, vec![switch(1, 0, 1)])]);
        match counter {
            "overrun" => result.stats[1].per_cpu[0].overrun = 1,
            "commit_overrun" => result.stats[1].per_cpu[0].commit_overrun = 1,
            "dropped_events" => result.stats[1].per_cpu[0].dropped_events = 1,
            "overwrite" => result.details[0].overwrite = 1,
            _ => unreachable!(),
        }
        write_fixture(&source, result);

        let error = kat_datasource::import_hitrace(
            &source,
            DatasetWriteTarget::write_to_empty(&dataset),
            |_| Ok(()),
        )
        .expect_err("loss evidence is rejected");
        let message = format!("{error:?}");
        assert!(message.contains(counter), "{counter}: {message}");
        assert!(!dataset.join(".kat-dataset").exists());
    }
}

#[test]
fn capture_damage_is_irrelevant_without_supported_ftrace_events() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("capture.htrace");
    let dataset = root.path().join("dataset");
    let mut result = complete_result("boot", vec![detail(0, Vec::new())]);
    result.stats[1].per_cpu[0].overrun = 1;
    result.details[0].overwrite = 1;
    result.stats.push(result.stats[1].clone());
    write_fixture(&source, result);

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(&dataset),
        |_| Ok(()),
    )
    .expect("capture metadata is ignored when no supported ftrace event exists");
    let tables = kat_datasource::inspect_dataset(&dataset)
        .expect("Dataset is inspectable")
        .tables()
        .iter()
        .map(|table| table.name().to_owned())
        .collect::<Vec<_>>();
    for fact in ["clock_domain", "clock_snapshot"] {
        assert!(
            tables.iter().any(|table| table == fact),
            "{fact} is preserved"
        );
    }
    assert!(
        !tables.iter().any(|table| table == "sched_switch"),
        "no supported switch fact is generated"
    );
    for relational in [
        "trace_plugin_result",
        "trace_plugin_result_ftrace_cpu_detail",
    ] {
        assert!(
            tables.iter().any(|table| table == relational),
            "{relational} is added"
        );
    }
}

#[test]
fn reported_ftrace_clock_is_validated_without_supported_events() {
    let cases = [
        {
            let mut result = complete_result("boot", vec![detail(0, Vec::new())]);
            result.stats[1].trace_clock = "future".to_owned();
            (result, "unsupported Hitrace trace clock")
        },
        {
            let mut result = complete_result("boot", vec![detail(0, Vec::new())]);
            result.stats[1].trace_clock = "local".to_owned();
            (result, "conflicting ftrace clocks")
        },
    ];

    for (result, expected) in cases {
        let root = tempdir().expect("tempdir");
        let source = root.path().join("capture.htrace");
        let dataset = root.path().join("dataset");
        write_fixture(&source, result);

        let error = kat_datasource::import_hitrace(
            &source,
            DatasetWriteTarget::write_to_empty(&dataset),
            |_| Ok(()),
        )
        .expect_err("invalid reported clock is rejected");
        let message = format!("{error:?}");
        assert!(message.contains(expected), "{expected}: {message}");
        assert!(!dataset.join(".kat-dataset").exists());
    }
}

#[test]
fn missing_ftrace_clock_is_allowed_without_supported_events() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("capture.htrace");
    let mut result = complete_result("", vec![detail(0, Vec::new())]);
    result.stats.push(result.stats[1].clone());
    write_fixture(&source, result);

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(root.path().join("dataset")),
        |_| Ok(()),
    )
    .expect("ftrace clock is optional when no supported event exists");
}

#[test]
fn trace_end_statistics_may_cover_cpus_without_detail_pages() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("capture.htrace");
    let mut result = complete_result("boot", vec![detail(0, vec![switch(1, 0, 1)])]);
    result.stats[1].per_cpu.push(cpu_stats(1));
    write_fixture(&source, result);

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(root.path().join("dataset")),
        |_| Ok(()),
    )
    .expect("TRACE_END CPU statistics cover details; additional CPUs are accepted");
}

#[test]
fn capture_requires_one_complete_end_snapshot_and_one_clock() {
    let cases = [
        {
            let mut result = complete_result("boot", vec![detail(0, vec![switch(1, 0, 1)])]);
            result.stats.retain(|stats| stats.status == 0);
            (result, "no TRACE_END")
        },
        {
            let mut result = complete_result("boot", vec![detail(0, vec![switch(1, 0, 1)])]);
            result.stats.push(result.stats[1].clone());
            (result, "duplicate ftrace TRACE_END")
        },
        {
            let mut result = complete_result("boot", vec![detail(0, vec![switch(1, 0, 1)])]);
            result.stats[1].trace_clock = "local".to_owned();
            (result, "conflicting ftrace clocks")
        },
        {
            let mut result = complete_result(
                "boot",
                vec![detail(0, vec![switch(1, 0, 1)]), detail(1, Vec::new())],
            );
            result.stats[1].per_cpu.retain(|stats| stats.cpu == 0);
            (result, "missing CPU 1")
        },
    ];

    for (result, expected) in cases {
        let root = tempdir().expect("tempdir");
        let source = root.path().join("capture.htrace");
        write_fixture(&source, result);
        let error = kat_datasource::import_hitrace(
            &source,
            DatasetWriteTarget::write_to_empty(root.path().join("dataset")),
            |_| Ok(()),
        )
        .expect_err("incomplete capture is rejected");
        let message = format!("{error:?}");
        assert!(message.contains(expected), "{expected}: {message}");
    }
}

#[test]
fn trace_start_loss_counters_are_not_used_as_the_capture_baseline() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("capture.htrace");
    let mut result = complete_result("boot", vec![detail(0, vec![switch(1, 0, 1)])]);
    result.stats[0].per_cpu[0].overrun = 100;
    result.stats[0].per_cpu[0].commit_overrun = 100;
    result.stats[0].per_cpu[0].dropped_events = 100;
    write_fixture(&source, result);

    kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(root.path().join("dataset")),
        |_| Ok(()),
    )
    .expect("TRACE_START counters are ignored");
}

#[test]
#[ignore = "requires KAT_REAL_HITRACE to name a real OpenHarmony zero-loss capture"]
fn real_openharmony_capture_smoke() {
    let source = PathBuf::from(
        std::env::var_os("KAT_REAL_HITRACE")
            .expect("set KAT_REAL_HITRACE to a real OpenHarmony capture"),
    );
    let root = tempdir().expect("tempdir");
    let imported = kat_datasource::import_hitrace(
        &source,
        DatasetWriteTarget::write_to_empty(root.path().join("dataset")),
        |_| Ok(()),
    )
    .expect("real OpenHarmony capture imports");

    let inspection = kat_datasource::inspect_dataset(imported.path()).expect("Dataset inspects");
    for table in [
        "clock_domain",
        "clock_snapshot",
        "sched_switch",
        "trace_plugin_result_clocks_detail",
    ] {
        assert!(
            inspection
                .tables()
                .iter()
                .any(|candidate| candidate.name() == table),
            "{table} should exist in the real capture"
        );
        let row_count = batches(&imported.path().join(format!("tables/{table}.parquet")))
            .iter()
            .map(arrow_array::RecordBatch::num_rows)
            .sum::<usize>();
        assert!(row_count > 0, "{table} should contain real rows");
    }
    for table in [
        "trace_plugin_result",
        "trace_plugin_result_ftrace_cpu_detail",
        "trace_plugin_result_ftrace_cpu_detail_event",
    ] {
        assert!(
            inspection
                .tables()
                .iter()
                .any(|candidate| candidate.name() == table),
            "{table} should coexist with source facts"
        );
    }

    let detail_path = imported
        .path()
        .join("tables/trace_plugin_result_ftrace_cpu_detail.parquet");
    let detail_keys = batches(&detail_path)
        .into_iter()
        .flat_map(|batch| {
            let sources = batch
                .column_by_name("source_index")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap()
                .clone();
            let rows = batch
                .column_by_name("row_index")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap()
                .clone();
            (0..batch.num_rows()).map(move |index| (sources.value(index), rows.value(index)))
        })
        .collect::<HashSet<_>>();

    let event_path = imported
        .path()
        .join("tables/trace_plugin_result_ftrace_cpu_detail_event.parquet");
    let mut event_count = 0usize;
    for batch in batches(&event_path) {
        let sources = batch
            .column_by_name("source_index")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let parents = batch
            .column_by_name("parent_index")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        for index in 0..batch.num_rows() {
            assert!(!parents.is_null(index), "event row should have a parent");
            assert!(
                detail_keys.contains(&(sources.value(index), parents.value(index))),
                "event row should reference an existing cpu detail row"
            );
            event_count += 1;
        }
    }
    assert!(event_count > 0, "real capture should contain ftrace events");
}

fn batches(path: &Path) -> Vec<arrow_array::RecordBatch> {
    ParquetRecordBatchReaderBuilder::try_new(fs::File::open(path).expect("Parquet file opens"))
        .expect("Parquet metadata reads")
        .build()
        .expect("Parquet reader builds")
        .collect::<Result<Vec<_>, _>>()
        .expect("Parquet batches read")
}
