use std::{fs, fs::File, path::Path};

use arrow_array::{Array, Int32Array, StringArray, UInt64Array};
use flate2::read::GzDecoder;
use kat_datasource::{DatasetWriteTarget, TextFtraceClock, import_text_ftrace, inspect_dataset};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

fn row_count(path: &Path) -> usize {
    ParquetRecordBatchReaderBuilder::try_new(File::open(path).unwrap())
        .unwrap()
        .build()
        .unwrap()
        .map(|batch| batch.unwrap().num_rows())
        .sum()
}

#[test]
fn text_ftrace_uses_the_canonical_trace_plugin_result_relations() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("capture.ftrace");
    let dataset = temp.path().join("dataset");
    fs::write(
        &source,
        "worker-7 (-------) [002] d.... 2.5: sched_switch: prev_comm=worker prev_pid=7 prev_prio=120 prev_state=S ==> next_comm=target next_pid=8 next_prio=100\n",
    )
    .unwrap();

    import_text_ftrace(
        &source,
        TextFtraceClock::Monotonic,
        DatasetWriteTarget::write_to_empty(&dataset),
    )
    .unwrap();

    let tables = inspect_dataset(&dataset)
        .unwrap()
        .tables()
        .iter()
        .map(|table| table.name().to_owned())
        .collect::<Vec<_>>();
    assert!(tables.iter().any(|name| name == "trace_plugin_result"));
    assert!(
        tables.iter().any(|name| {
            name == "trace_plugin_result_ftrace_cpu_detail_event_sched_switch_format"
        })
    );
    assert!(
        !tables
            .iter()
            .any(|name| name == "profiler_payload_occurrence")
    );
    assert!(
        !tables
            .iter()
            .any(|name| name == "ftrace_event_sched_switch")
    );

    let batch = ParquetRecordBatchReaderBuilder::try_new(
        File::open(dataset.join("tables/trace_plugin_result.parquet")).unwrap(),
    )
    .unwrap()
    .build()
    .unwrap()
    .next()
    .unwrap()
    .unwrap();
    let parent = batch
        .column_by_name("_kat_parent_row_id")
        .unwrap()
        .as_any()
        .downcast_ref::<UInt64Array>()
        .unwrap();
    assert!(parent.is_null(0));
}

#[test]
fn unknown_events_are_reported_and_absent_events_do_not_create_tables() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("capture.ftrace");
    let dataset = temp.path().join("dataset");
    fs::write(
        &source,
        concat!(
            " worker-name-7 (-------) [002] d.... 2.5: sched_wakeup: comm=target pid=8 prio=120 target_cpu=003\n",
            " worker-name-7 (-------) [002] d.... 2.6: future_event: id=1 name=test\n",
            " worker-name-7 (-------) [002] d.... 2.7: future_event: id=2 name=test\n",
        ),
    )
    .unwrap();

    let imported = import_text_ftrace(
        &source,
        TextFtraceClock::Monotonic,
        DatasetWriteTarget::write_to_empty(&dataset),
    )
    .unwrap();

    let unsupported = imported.unsupported_events();
    assert_eq!(unsupported.len(), 1);
    assert_eq!(unsupported[0].name(), "future_event");
    assert_eq!(unsupported[0].count(), 2);
    assert_eq!(unsupported[0].first_line(), 2);
    assert_eq!(
        inspect_dataset(&dataset)
            .unwrap()
            .tables()
            .iter()
            .map(|table| table.name())
            .collect::<Vec<_>>(),
        [
            "clock_domain",
            "protobuf_enum_symbol",
            "trace_plugin_config",
            "trace_plugin_result",
            "trace_plugin_result_ftrace_cpu_detail",
            "trace_plugin_result_ftrace_cpu_detail_event",
            "trace_plugin_result_ftrace_cpu_detail_event_sched_wakeup_format",
        ]
    );
    let batch = ParquetRecordBatchReaderBuilder::try_new(
        File::open(dataset.join("tables/trace_plugin_result_ftrace_cpu_detail_event.parquet"))
            .unwrap(),
    )
    .unwrap()
    .build()
    .unwrap()
    .next()
    .unwrap()
    .unwrap();
    let uint64 = |name| {
        batch
            .column_by_name(name)
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .value(0)
    };
    assert_eq!(uint64("timestamp"), 2_500_000_000);
    assert_eq!(
        batch
            .column_by_name("comm")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "worker-name"
    );
    assert!(
        batch
            .column_by_name("tgid")
            .unwrap()
            .as_any()
            .downcast_ref::<Int32Array>()
            .unwrap()
            .is_null(0)
    );
}

#[test]
fn malformed_supported_event_does_not_publish_or_replace_a_dataset() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("capture.ftrace");
    let dataset = temp.path().join("dataset");
    fs::write(
        &source,
        "worker-7 (-------) [002] d.... 2.5: sched_wakeup: comm=target pid=not-a-pid prio=120 target_cpu=003\n",
    )
    .unwrap();
    fs::create_dir(&dataset).unwrap();
    fs::write(dataset.join("sentinel"), "unchanged").unwrap();

    let error = import_text_ftrace(
        &source,
        TextFtraceClock::Boottime,
        DatasetWriteTarget::permanently_replace_all_contents(&dataset),
    )
    .unwrap_err();

    assert!(error.to_string().contains("line 1"), "{error:#}");
    let compatibility = error.compatibility().expect("known field incompatibility");
    assert_eq!(compatibility.event(), "sched_wakeup");
    assert_eq!(compatibility.field(), "pid");
    assert_eq!(compatibility.line(), 1);
    assert_eq!(compatibility.reason(), "invalid signed 32-bit integer");
    assert_eq!(
        fs::read_to_string(dataset.join("sentinel")).unwrap(),
        "unchanged"
    );
}

#[test]
fn malformed_additional_event_does_not_publish_or_replace_a_dataset() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("capture.ftrace");
    let dataset = temp.path().join("dataset");
    fs::write(
        &source,
        "worker-7 (-------) [002] d.... 2.5: irq_handler_entry: irq=not-an-irq name=test\n",
    )
    .unwrap();
    fs::create_dir(&dataset).unwrap();
    fs::write(dataset.join("sentinel"), "unchanged").unwrap();

    let error = import_text_ftrace(
        &source,
        TextFtraceClock::Boottime,
        DatasetWriteTarget::permanently_replace_all_contents(&dataset),
    )
    .unwrap_err();

    assert!(error.to_string().contains("line 1"), "{error:#}");
    assert_eq!(
        fs::read_to_string(dataset.join("sentinel")).unwrap(),
        "unchanged"
    );
}

#[test]
fn header_only_text_is_rejected_without_publishing_a_dataset() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("empty.ftrace");
    let dataset = temp.path().join("dataset");
    fs::write(
        &source,
        "# tracer: nop\n# entries-in-buffer/entries-written: 0/0\n",
    )
    .unwrap();

    let error = import_text_ftrace(
        &source,
        TextFtraceClock::Boottime,
        DatasetWriteTarget::write_to_empty(&dataset),
    )
    .unwrap_err();

    assert!(
        error.to_string().contains("contains no event records"),
        "{error:#}"
    );
    assert!(!dataset.exists());
}

#[test]
fn marker_content_preserves_trailing_spaces() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("marker.ftrace");
    let dataset = temp.path().join("dataset");
    fs::write(
        &source,
        "hitrace-10 ( 10) [003] ..... 1.0: tracing_mark_write: marker payload  \n",
    )
    .unwrap();

    import_text_ftrace(
        &source,
        TextFtraceClock::FtraceGlobal,
        DatasetWriteTarget::write_to_empty(&dataset),
    )
    .unwrap();

    let batch = ParquetRecordBatchReaderBuilder::try_new(
        File::open(
            dataset.join("tables/trace_plugin_result_ftrace_cpu_detail_event_print_format.parquet"),
        )
        .unwrap(),
    )
    .unwrap()
    .build()
    .unwrap()
    .next()
    .unwrap()
    .unwrap();
    assert_eq!(
        batch
            .column_by_name("buf")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "marker payload  "
    );
}

#[test]
fn captured_text_ftrace_preserves_all_four_event_counts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let fixture = root.join("trace/kat_hitrace_text.ftrace.gz");
    assert!(
        fixture.is_file(),
        "missing captured fixture {}",
        fixture.display()
    );
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("kat_hitrace_text.ftrace");
    let mut decoder = GzDecoder::new(File::open(&fixture).unwrap());
    let mut decoded = File::create(&source).unwrap();
    std::io::copy(&mut decoder, &mut decoded).unwrap();
    let dataset = temp.path().join("dataset");

    import_text_ftrace(
        source,
        TextFtraceClock::Boottime,
        DatasetWriteTarget::write_to_empty(&dataset),
    )
    .unwrap();

    for (table, expected) in [
        (
            "trace_plugin_result_ftrace_cpu_detail_event_sched_switch_format",
            6_005,
        ),
        (
            "trace_plugin_result_ftrace_cpu_detail_event_sched_wakeup_format",
            3_032,
        ),
        (
            "trace_plugin_result_ftrace_cpu_detail_event_sched_wakeup_new_format",
            4,
        ),
        (
            "trace_plugin_result_ftrace_cpu_detail_event_print_format",
            2,
        ),
    ] {
        assert_eq!(
            row_count(&dataset.join("tables").join(format!("{table}.parquet"))),
            expected,
            "unexpected row count for {table}"
        );
    }
}

#[test]
fn text_ftrace_streams_across_the_proto_spool_boundary_in_source_order() {
    const EVENT_COUNT: usize = 8_193;
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("capture.ftrace");
    let dataset = temp.path().join("dataset");
    let mut input = String::new();
    for index in 0..EVENT_COUNT {
        let timestamp = format!("1.{index:09}");
        if index % 2 == 0 {
            input.push_str(&format!(
                "worker-7 ( 7) [000] ..... {timestamp}: tracing_mark_write: marker-{index}\n"
            ));
        } else {
            input.push_str(&format!(
                "worker-7 ( 7) [000] ..... {timestamp}: sched_wakeup: comm=target pid=8 prio=120 target_cpu=000\n"
            ));
        }
    }
    fs::write(&source, input).unwrap();

    import_text_ftrace(
        &source,
        TextFtraceClock::Monotonic,
        DatasetWriteTarget::write_to_empty(&dataset),
    )
    .unwrap();

    assert_eq!(
        row_count(&dataset.join("tables/trace_plugin_result.parquet")),
        1
    );
    assert_eq!(
        row_count(&dataset.join("tables/trace_plugin_result_ftrace_cpu_detail.parquet")),
        1
    );
    let path = dataset.join("tables/trace_plugin_result_ftrace_cpu_detail_event.parquet");
    let reader = ParquetRecordBatchReaderBuilder::try_new(File::open(path).unwrap())
        .unwrap()
        .build()
        .unwrap();
    let mut expected = 0_u64;
    for batch in reader {
        let batch = batch.unwrap();
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
            assert_eq!(indices.value(row), expected);
            assert_eq!(timestamps.value(row), 1_000_000_000 + expected);
            expected += 1;
        }
    }
    assert_eq!(expected, EVENT_COUNT as u64);
}

#[test]
fn malformed_event_after_the_proto_spool_boundary_does_not_publish_a_dataset() {
    const VALID_EVENT_COUNT: usize = 8_193;
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("capture.ftrace");
    let dataset = temp.path().join("dataset");
    let mut input = String::new();
    for index in 0..VALID_EVENT_COUNT {
        input.push_str(&format!(
            "worker-7 ( 7) [000] ..... 1.{index:09}: tracing_mark_write: marker-{index}\n"
        ));
    }
    input.push_str(
        "worker-7 ( 7) [000] ..... 2.0: sched_wakeup: comm=target pid=invalid prio=120 target_cpu=000\n",
    );
    fs::write(&source, input).unwrap();

    let error = import_text_ftrace(
        &source,
        TextFtraceClock::Monotonic,
        DatasetWriteTarget::write_to_empty(&dataset),
    )
    .unwrap_err();

    assert!(
        error.to_string().contains("line 8194"),
        "unexpected error: {error:#}"
    );
    assert!(!dataset.exists());
}
