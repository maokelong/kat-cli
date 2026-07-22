use std::{
    fs,
    path::{Path, PathBuf},
};

use arrow_array::{Int32Array, StringArray, UInt32Array, UInt64Array};
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

fn write_fixture(path: &Path, result: TraceResult) {
    fs::write(path, fixture(result)).expect("Hitrace fixture is written");
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
    assert_eq!(tables, ["clock_domain", "clock_snapshot", "sched_switch"]);

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
fn clock_and_thread_continuity_damage_fail_before_target_mutation() {
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
        assert_eq!(
            fs::read_to_string(dataset.join("sentinel")).expect("sentinel remains"),
            "unchanged"
        );
    }
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
        assert!(
            !dataset.exists(),
            "invalid capture must not publish a Dataset"
        );
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
    assert_eq!(tables, ["clock_domain", "clock_snapshot"]);
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
        assert!(
            !dataset.exists(),
            "invalid clock must not publish a Dataset"
        );
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
    assert!(
        inspection
            .tables()
            .iter()
            .any(|table| table.name() == "sched_switch")
    );
}

fn batches(path: &Path) -> Vec<arrow_array::RecordBatch> {
    ParquetRecordBatchReaderBuilder::try_new(fs::File::open(path).expect("Parquet file opens"))
        .expect("Parquet metadata reads")
        .build()
        .expect("Parquet reader builds")
        .collect::<Result<Vec<_>, _>>()
        .expect("Parquet batches read")
}
