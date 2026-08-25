use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use arrow_array::UInt64Array;
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

fn validate_ftrace_integrity_contract(result: &TraceResult) -> Result<(), String> {
    let mut clocks = BTreeSet::new();
    let mut end_stats = None;
    let mut integrity_error = None;
    for stats in &result.stats {
        let clock = stats.trace_clock.trim();
        if !clock.is_empty() {
            if !matches!(clock, "boot" | "mono" | "global" | "local") {
                return Err(format!("unsupported Hitrace trace clock {clock:?}"));
            }
            clocks.insert(clock);
        }
        if integrity_error.is_some() {
            continue;
        }
        if !matches!(stats.status, 0 | 1) {
            integrity_error = Some(format!("invalid ftrace stats status {}", stats.status));
            continue;
        }
        if stats.status != 1 {
            continue;
        }
        if end_stats.is_some() {
            integrity_error = Some("duplicate ftrace TRACE_END statistics".to_owned());
            continue;
        }
        let mut cpus = HashSet::new();
        for cpu_stats in &stats.per_cpu {
            let cpu = u32::try_from(cpu_stats.cpu).map_err(|_| {
                format!(
                    "ftrace CPU id {} cannot be represented as UInt32",
                    cpu_stats.cpu
                )
            })?;
            if !cpus.insert(cpu) {
                integrity_error = Some(format!(
                    "duplicate ftrace TRACE_END statistics for CPU {cpu}"
                ));
                break;
            }
            for (name, value) in [
                ("overrun", cpu_stats.overrun),
                ("commit_overrun", cpu_stats.commit_overrun),
                ("dropped_events", cpu_stats.dropped_events),
            ] {
                if value != 0 {
                    integrity_error = Some(format!(
                        "ftrace capture lost events on CPU {cpu}: {name}={value}"
                    ));
                    break;
                }
            }
        }
        end_stats = Some(cpus);
    }

    let mut last_switch = HashMap::new();
    let mut detail_cpus = Vec::new();
    let mut first_overwrite = None;
    let mut has_supported_event = false;
    for (detail_sequence, detail) in result.details.iter().enumerate() {
        detail_cpus.push((detail_sequence, detail.cpu));
        if detail.overwrite != 0 && first_overwrite.is_none() {
            first_overwrite = Some((detail_sequence, detail.cpu, detail.overwrite));
        }
        for event in &detail.events {
            let Some(switch) = &event.switch else {
                continue;
            };
            has_supported_event = true;
            if let Some((timestamp, next_id)) = last_switch.get(&detail.cpu) {
                if event.timestamp < *timestamp {
                    return Err(format!(
                        "sched_switch clock went backwards on CPU {}",
                        detail.cpu
                    ));
                }
                if switch.previous_id != *next_id {
                    return Err(format!(
                        "sched_switch thread continuity is broken on CPU {}",
                        detail.cpu
                    ));
                }
            }
            last_switch.insert(detail.cpu, (event.timestamp, switch.next_id));
        }
    }

    if clocks.len() > 1 {
        return Err("Hitrace reports conflicting ftrace clocks".to_owned());
    }
    if !has_supported_event {
        return Ok(());
    }
    if clocks.is_empty() {
        return Err("Hitrace sched_switch data has no ftrace clock".to_owned());
    }
    if let Some(error) = integrity_error {
        return Err(error);
    }
    let end_stats = end_stats
        .ok_or_else(|| "Hitrace sched_switch data has no TRACE_END statistics".to_owned())?;
    let first_missing = detail_cpus
        .into_iter()
        .find(|(_, cpu)| !end_stats.contains(cpu));
    match (first_overwrite, first_missing) {
        (Some((sequence, cpu, overwrite)), Some((missing_sequence, _)))
            if sequence <= missing_sequence =>
        {
            Err(format!(
                "ftrace page overwrite is nonzero on CPU {cpu}: {overwrite}"
            ))
        }
        (_, Some((_, cpu))) => Err(format!(
            "Hitrace TRACE_END statistics are missing CPU {cpu}"
        )),
        (Some((_, cpu, overwrite)), None) => Err(format!(
            "ftrace page overwrite is nonzero on CPU {cpu}: {overwrite}"
        )),
        (None, None) => Ok(()),
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
fn import_publishes_descriptor_ftrace_relations_in_source_order() {
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
    for required in [
        "profiler_payload_occurrence",
        "trace_plugin_result",
        "trace_plugin_result_clocks_detail",
        "trace_plugin_result_ftrace_cpu_detail",
        "trace_plugin_result_ftrace_cpu_detail_event",
        "trace_plugin_result_ftrace_cpu_detail_event_sched_switch_format",
    ] {
        assert!(tables.iter().any(|table| table == required), "{tables:?}");
    }
    assert!(!tables.iter().any(|table| table == "sched_switch"));

    let mut event_rows = Vec::new();
    for batch in
        batches(&dataset.join("tables/trace_plugin_result_ftrace_cpu_detail_event.parquet"))
    {
        let indices = batch
            .column_by_name("_kat_repeated_index")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let timestamps = batch
            .column_by_name("timestamp")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        for row in 0..batch.num_rows() {
            event_rows.push((indices.value(row), timestamps.value(row)));
        }
    }
    assert_eq!(event_rows, [(0, 10), (1, 10), (0, 7)]);
    assert_eq!(
        batches(&dataset.join("tables/clock_snapshot.parquet"))
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        6
    );
}

#[test]
fn import_batches_descriptor_events_without_losing_repeated_order() {
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
    let batches =
        batches(&dataset.join("tables/trace_plugin_result_ftrace_cpu_detail_event.parquet"));
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        8193
    );
    let last = batches.last().expect("at least one switch batch");
    let sequences = last
        .column_by_name("_kat_repeated_index")
        .unwrap()
        .as_any()
        .downcast_ref::<UInt64Array>()
        .unwrap();
    assert_eq!(sequences.value(sequences.len() - 1), 8192);
}

#[test]
fn import_batches_descriptor_roots_without_changing_source_order() {
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

    let root_batches = batches(&dataset.join("tables/trace_plugin_result.parquet"));
    assert!(root_batches.len() >= 2, "root rows cross a buffer boundary");
    assert_eq!(
        root_batches
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        8193
    );
    let first = root_batches.first().unwrap();
    let last = root_batches.last().unwrap();
    assert_eq!(
        first
            .column_by_name("_kat_row_id")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .value(0),
        0
    );
    let last_ids = last
        .column_by_name("_kat_row_id")
        .unwrap()
        .as_any()
        .downcast_ref::<UInt64Array>()
        .unwrap();
    assert_eq!(last_ids.value(last_ids.len() - 1), 8192);
}

#[test]
fn ftrace_contract_rejects_clock_and_thread_continuity_damage() {
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
        let result = complete_result("boot", vec![detail(0, events)]);
        let message = validate_ftrace_integrity_contract(&result)
            .expect_err("contract rejects damaged capture");
        assert!(message.contains(expected), "{message}");
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
fn ftrace_contract_rejects_every_loss_evidence() {
    for counter in ["overrun", "commit_overrun", "dropped_events", "overwrite"] {
        let mut result = complete_result("global", vec![detail(0, vec![switch(1, 0, 1)])]);
        match counter {
            "overrun" => result.stats[1].per_cpu[0].overrun = 1,
            "commit_overrun" => result.stats[1].per_cpu[0].commit_overrun = 1,
            "dropped_events" => result.stats[1].per_cpu[0].dropped_events = 1,
            "overwrite" => result.details[0].overwrite = 1,
            _ => unreachable!(),
        }
        let message = validate_ftrace_integrity_contract(&result)
            .expect_err("contract rejects loss evidence");
        assert!(message.contains(counter), "{counter}: {message}");
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
    assert!(tables.iter().any(|table| table == "trace_plugin_result"));
    assert!(
        tables
            .iter()
            .any(|table| table == "trace_plugin_result_ftrace_cpu_stats_per_cpu_stats")
    );
    assert!(!tables.iter().any(|table| table == "sched_switch"));
}

#[test]
fn ftrace_contract_validates_reported_clock_without_supported_events() {
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
        let message = validate_ftrace_integrity_contract(&result)
            .expect_err("contract rejects invalid reported clock");
        assert!(message.contains(expected), "{expected}: {message}");
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
fn ftrace_contract_requires_one_complete_end_snapshot_and_one_clock() {
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
        let message = validate_ftrace_integrity_contract(&result)
            .expect_err("contract rejects incomplete capture");
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
fn real_openharmony_descriptor_capture_smoke() {
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
            .any(|table| table.name() == "trace_plugin_result")
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
