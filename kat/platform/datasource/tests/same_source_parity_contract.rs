use std::{collections::BTreeMap, fs, fs::File, path::Path};

use arrow_array::{Array, Int32Array, StringArray, StructArray, UInt32Array, UInt64Array};
use kat_datasource::{DatasetWriteTarget, TextFtraceClock};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use prost::Message;

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
struct ResultMsg {
    #[prost(message, repeated, tag = "1")]
    stats: Vec<Stats>,
    #[prost(message, repeated, tag = "2")]
    details: Vec<Detail>,
}
#[derive(Clone, PartialEq, Message)]
struct Stats {
    #[prost(int32, tag = "1")]
    status: i32,
    #[prost(message, repeated, tag = "2")]
    per_cpu: Vec<PerCpu>,
    #[prost(string, tag = "3")]
    clock: String,
}
#[derive(Clone, Copy, PartialEq, Message)]
struct PerCpu {
    #[prost(uint64, tag = "1")]
    cpu: u64,
}
#[derive(Clone, PartialEq, Message)]
struct Detail {
    #[prost(uint32, tag = "1")]
    cpu: u32,
    #[prost(message, repeated, tag = "2")]
    events: Vec<Event>,
    #[prost(uint64, optional, tag = "3")]
    overwrite: Option<u64>,
}
#[derive(Clone, PartialEq, Message)]
struct Event {
    #[prost(uint64, tag = "1")]
    timestamp: u64,
    #[prost(int32, optional, tag = "2")]
    tgid: Option<i32>,
    #[prost(string, tag = "3")]
    comm: String,
    #[prost(message, optional, tag = "50")]
    common: Option<Common>,
    #[prost(message, optional, tag = "2417")]
    switch: Option<Switch>,
}
#[derive(Clone, Copy, PartialEq, Message)]
struct Common {
    #[prost(int32, tag = "4")]
    pid: i32,
}
#[derive(Clone, PartialEq, Message)]
struct Switch {
    #[prost(string, tag = "1")]
    prev_comm: String,
    #[prost(int32, tag = "2")]
    prev_pid: i32,
    #[prost(int32, tag = "3")]
    prev_prio: i32,
    #[prost(uint64, tag = "4")]
    prev_state: u64,
    #[prost(string, tag = "5")]
    next_comm: String,
    #[prost(int32, tag = "6")]
    next_pid: i32,
    #[prost(int32, tag = "7")]
    next_prio: i32,
}

#[derive(Debug, Eq, PartialEq)]
struct Row {
    timestamp: u64,
    tgid: Option<i32>,
    comm: String,
    pid: i32,
    payload: (String, i32, i32, u64, String, i32, i32),
}

fn event(
    timestamp: u64,
    emitter: (&str, i32, i32),
    payload: (&str, i32, i32, u64, &str, i32, i32),
) -> Event {
    Event {
        timestamp,
        tgid: Some(emitter.2),
        comm: emitter.0.into(),
        common: Some(Common { pid: emitter.1 }),
        switch: Some(Switch {
            prev_comm: payload.0.into(),
            prev_pid: payload.1,
            prev_prio: payload.2,
            prev_state: payload.3,
            next_comm: payload.4.into(),
            next_pid: payload.5,
            next_prio: payload.6,
        }),
    }
}

fn write_htrace(path: &Path) {
    let details = vec![
        Detail {
            cpu: 0,
            overwrite: None,
            events: vec![
                event(
                    1_000_000_001,
                    ("worker", 7, 70),
                    ("worker", 7, 120, 1, "target", 8, 100),
                ),
                event(
                    1_000_000_003,
                    ("target", 8, 80),
                    ("target", 8, 100, 0, "worker", 7, 120),
                ),
            ],
        },
        Detail {
            cpu: 1,
            overwrite: None,
            events: vec![event(
                1_000_000_002,
                ("helper", 9, 90),
                ("helper", 9, 110, 1, "idle", 0, 120),
            )],
        },
    ];
    let cpus = vec![PerCpu { cpu: 0 }, PerCpu { cpu: 1 }];
    let result = ResultMsg {
        stats: vec![
            Stats {
                status: 0,
                per_cpu: cpus.clone(),
                clock: "local".into(),
            },
            Stats {
                status: 1,
                per_cpu: cpus,
                clock: "local".into(),
            },
        ],
        details,
    };
    let envelope = Envelope {
        name: "ftrace-plugin".into(),
        data: result.encode_to_vec(),
    }
    .encode_to_vec();
    let mut bytes = vec![0; HEADER_SIZE];
    bytes[0..8].copy_from_slice(&HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((HEADER_SIZE + 4 + envelope.len()) as u64).to_le_bytes());
    for (offset, value) in [60, 68, 76, 84, 92, 100].into_iter().zip(1_u64..=6) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&(envelope.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&envelope);
    fs::write(path, bytes).unwrap();
}

fn batches(path: &Path) -> Vec<arrow_array::RecordBatch> {
    ParquetRecordBatchReaderBuilder::try_new(File::open(path).unwrap())
        .unwrap()
        .build()
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn snapshot(dataset: &Path) -> BTreeMap<(u32, u64), Row> {
    let tables = dataset.join("tables");
    let mut details = BTreeMap::new();
    for batch in batches(&tables.join("trace_plugin_result_ftrace_cpu_detail.parquet")) {
        let ids = batch
            .column_by_name("_kat_row_id")
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
        for row in 0..batch.num_rows() {
            details.insert(ids.value(row), cpus.value(row));
        }
    }
    let mut payloads = BTreeMap::new();
    for batch in batches(
        &tables.join("trace_plugin_result_ftrace_cpu_detail_event_sched_switch_format.parquet"),
    ) {
        let u64c = |n| {
            batch
                .column_by_name(n)
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap()
        };
        let i32c = |n| {
            batch
                .column_by_name(n)
                .unwrap()
                .as_any()
                .downcast_ref::<Int32Array>()
                .unwrap()
        };
        let strc = |n| {
            batch
                .column_by_name(n)
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap()
        };
        for row in 0..batch.num_rows() {
            payloads.insert(
                u64c("_kat_parent_row_id").value(row),
                (
                    strc("prev_comm").value(row).into(),
                    i32c("prev_pid").value(row),
                    i32c("prev_prio").value(row),
                    u64c("prev_state").value(row),
                    strc("next_comm").value(row).into(),
                    i32c("next_pid").value(row),
                    i32c("next_prio").value(row),
                ),
            );
        }
    }
    let mut result = BTreeMap::new();
    for batch in batches(&tables.join("trace_plugin_result_ftrace_cpu_detail_event.parquet")) {
        let u64c = |n| {
            batch
                .column_by_name(n)
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap()
        };
        let i32c = |n| {
            batch
                .column_by_name(n)
                .unwrap()
                .as_any()
                .downcast_ref::<Int32Array>()
                .unwrap()
        };
        let strc = |n| {
            batch
                .column_by_name(n)
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap()
        };
        let common = batch
            .column_by_name("common_fields")
            .unwrap()
            .as_any()
            .downcast_ref::<StructArray>()
            .unwrap();
        let pids = common
            .column_by_name("pid")
            .unwrap()
            .as_any()
            .downcast_ref::<Int32Array>()
            .unwrap();
        for row in 0..batch.num_rows() {
            let id = u64c("_kat_row_id").value(row);
            let parent = u64c("_kat_parent_row_id").value(row);
            let tgid = i32c("tgid");
            result.insert(
                (details[&parent], u64c("_kat_repeated_index").value(row)),
                Row {
                    timestamp: u64c("timestamp").value(row),
                    tgid: (!tgid.is_null(row)).then(|| tgid.value(row)),
                    comm: strc("comm").value(row).into(),
                    pid: pids.value(row),
                    payload: payloads.remove(&id).unwrap(),
                },
            );
        }
    }
    result
}

#[test]
fn same_source_text_and_htrace_have_equal_shared_facts() {
    let root = tempfile::tempdir().unwrap();
    let text = root.path().join("same.ftrace");
    let binary = root.path().join("same.htrace");
    let text_dataset = root.path().join("text");
    let binary_dataset = root.path().join("binary");
    fs::write(&text, concat!(
        "worker-7 ( 70) [000] ..... 1.000000001: sched_switch: prev_comm=worker prev_pid=7 prev_prio=120 prev_state=S ==> next_comm=target next_pid=8 next_prio=100\n",
        "helper-9 ( 90) [001] ..... 1.000000002: sched_switch: prev_comm=helper prev_pid=9 prev_prio=110 prev_state=S ==> next_comm=idle next_pid=0 next_prio=120\n",
        "target-8 ( 80) [000] ..... 1.000000003: sched_switch: prev_comm=target prev_pid=8 prev_prio=100 prev_state=R ==> next_comm=worker next_pid=7 next_prio=120\n",
    )).unwrap();
    write_htrace(&binary);
    kat_datasource::import_text_ftrace(
        &text,
        TextFtraceClock::Monotonic,
        DatasetWriteTarget::write_to_empty(&text_dataset),
    )
    .unwrap();
    kat_datasource::import_hitrace(
        &binary,
        DatasetWriteTarget::write_to_empty(&binary_dataset),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(snapshot(&text_dataset), snapshot(&binary_dataset));
}
