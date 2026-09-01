use std::{fs, fs::File, process::Command};

use arrow_array::{Array, BooleanArray, Int32Array, StringArray, UInt32Array, UInt64Array};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

fn row_count(path: &std::path::Path) -> usize {
    ParquetRecordBatchReaderBuilder::try_new(File::open(path).unwrap())
        .unwrap()
        .build()
        .unwrap()
        .map(|batch| batch.unwrap().num_rows())
        .sum()
}

fn header(entries: u64, written: u64, cpus: u32, has_tgid: bool) -> String {
    let tgid = if has_tgid { "TGID    " } else { "" };
    format!(
        concat!(
            "# tracer: nop\n",
            "#\n",
            "# entries-in-buffer/entries-written: {entries}/{written}   #P:{cpus}\n",
            "#\n",
            "# _-----=> irqs-off/BH-disabled\n",
            "# / _----=> need-resched\n",
            "# | / _---=> hardirq/softirq\n",
            "# || / _--=> preempt-depth\n",
            "# ||| / _-=> migrate-disable\n",
            "# |||| /     delay\n",
            "# TASK-PID       {tgid}CPU#  |||||  TIMESTAMP  FUNCTION\n",
            "# | |            |       |   |||||     |         |\n",
        ),
        entries = entries,
        written = written,
        cpus = cpus,
        tgid = tgid,
    )
}

fn trace(events: &str, entries: u64, written: u64, cpus: u32, has_tgid: bool) -> String {
    format!("{}{events}", header(entries, written, cpus, has_tgid))
}

#[test]
fn converts_proto_root_and_payload_tables_with_unknown_sequence_gaps() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("trace.ftrace");
    let output = temp.path().join("parquet");
    fs::write(
        &input,
        trace(
            concat!(
            " Render Thread (IO)-42 (-------) [007] dN..2 1.000000001: sched_wakeup: comm=target pid=8 prio=120 target_cpu=003\n",
            " Render Thread (IO)-42 (-------) [007] dN..2 1.1: custom_event: value=1\n",
            "worker-name-9       (    123) [002] d.... 1.5: sched_wakeup: comm=second pid=9 prio=100 target_cpu=001\n",
        ),
            3,
            3,
            8,
            true,
        ),
    )
    .unwrap();

    ftrace2parquet::convert(&input, &output, "monotonic").unwrap();

    let root = ParquetRecordBatchReaderBuilder::try_new(
        File::open(output.join("text_ftrace_event.parquet")).unwrap(),
    )
    .unwrap()
    .build()
    .unwrap()
    .next()
    .unwrap()
    .unwrap();
    assert_eq!(root.num_rows(), 2);
    let occurrence = ParquetRecordBatchReaderBuilder::try_new(
        File::open(output.join("text_ftrace_event_occurrence.parquet")).unwrap(),
    )
    .unwrap()
    .build()
    .unwrap()
    .next()
    .unwrap()
    .unwrap();
    assert_eq!(
        occurrence
            .column_by_name("source_event_sequence")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .values(),
        &[0, 2]
    );
    assert_eq!(
        root.column_by_name("emitter_thread_name")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "Render Thread (IO)"
    );
    assert!(
        root.column_by_name("emitter_process_id")
            .unwrap()
            .as_any()
            .downcast_ref::<Int32Array>()
            .unwrap()
            .is_null(0)
    );
    assert!(
        output
            .join("text_ftrace_event_sched_wakeup.parquet")
            .is_file()
    );
    assert!(
        !output
            .join("text_ftrace_event_sched_switch.parquet")
            .exists()
    );
    let header = ParquetRecordBatchReaderBuilder::try_new(
        File::open(output.join("text_ftrace_header.parquet")).unwrap(),
    )
    .unwrap()
    .build()
    .unwrap()
    .next()
    .unwrap()
    .unwrap();
    assert_eq!(
        header
            .column_by_name("tracer")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "nop"
    );
    assert_eq!(
        header
            .column_by_name("entries_in_buffer")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .value(0),
        3
    );
    assert_eq!(
        header
            .column_by_name("entries_written")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .value(0),
        3
    );
    assert_eq!(
        header
            .column_by_name("cpu_count")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt32Array>()
            .unwrap()
            .value(0),
        8
    );
    assert!(
        header
            .column_by_name("has_tgid_column")
            .unwrap()
            .as_any()
            .downcast_ref::<BooleanArray>()
            .unwrap()
            .value(0)
    );
}

#[test]
fn creates_each_proto_oneof_table_only_when_observed() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("all.ftrace");
    let output = temp.path().join("parquet");
    fs::write(
        &input,
        trace(
            concat!(
            "worker-7 ( 7) [002] d.... 1.0: sched_switch: prev_comm=old prev_pid=7 prev_prio=120 prev_state=R+ ==> next_comm=new next_pid=8 next_prio=100\n",
            "worker-7 ( 7) [002] d.... 2.0: sched_wakeup: comm=a pid=8 prio=120 target_cpu=003\n",
            "worker-7 ( 7) [002] d.... 3.0: sched_wakeup_new: comm=b pid=9 prio=100 target_cpu=001\n",
            "worker-7 ( 7) [002] d.... 4.0: tracing_mark_write: marker payload  \n",
        ),
            4,
            4,
            4,
            true,
        ),
    )
    .unwrap();
    ftrace2parquet::convert(&input, &output, "boottime").unwrap();
    assert_eq!(row_count(&output.join("text_ftrace_event.parquet")), 4);
    for table in [
        "text_ftrace_event_sched_switch",
        "text_ftrace_event_sched_wakeup",
        "text_ftrace_event_sched_wakeup_new",
        "text_ftrace_event_tracing_mark_write",
    ] {
        assert_eq!(row_count(&output.join(format!("{table}.parquet"))), 1);
    }
}

#[test]
fn accepts_a_header_without_tgid_and_records_the_column_contract() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("without-tgid.ftrace");
    let output = temp.path().join("parquet");
    fs::write(
        &input,
        trace(
            "worker-7 [002] d.... 1.0: sched_wakeup: comm=target pid=8 prio=120 target_cpu=003\n",
            1,
            1,
            4,
            false,
        ),
    )
    .unwrap();

    ftrace2parquet::convert(&input, &output, "boottime").unwrap();

    let header = ParquetRecordBatchReaderBuilder::try_new(
        File::open(output.join("text_ftrace_header.parquet")).unwrap(),
    )
    .unwrap()
    .build()
    .unwrap()
    .next()
    .unwrap()
    .unwrap();
    assert!(
        !header
            .column_by_name("has_tgid_column")
            .unwrap()
            .as_any()
            .downcast_ref::<BooleanArray>()
            .unwrap()
            .value(0)
    );
    let root = ParquetRecordBatchReaderBuilder::try_new(
        File::open(output.join("text_ftrace_event.parquet")).unwrap(),
    )
    .unwrap()
    .build()
    .unwrap()
    .next()
    .unwrap()
    .unwrap();
    assert!(
        root.column_by_name("emitter_process_id")
            .unwrap()
            .as_any()
            .downcast_ref::<Int32Array>()
            .unwrap()
            .is_null(0)
    );
}

#[test]
fn rejects_malformed_or_inconsistent_headers_without_publication() {
    let event =
        "worker-7 ( 7) [002] d.... 1.0: sched_wakeup: comm=target pid=8 prio=120 target_cpu=003\n";
    let valid = trace(event, 1, 1, 4, true);
    let cases = [
        (
            "missing-tracer",
            valid.replacen("# tracer: nop\n", "", 1),
            "buffer header precedes tracer",
        ),
        (
            "duplicate-tracer",
            valid.replacen("# tracer: nop\n", "# tracer: nop\n# tracer: nop\n", 1),
            "duplicate tracer header",
        ),
        (
            "invalid-buffer-relation",
            valid.replacen("1/1", "2/1", 1),
            "entries-in-buffer 2 exceeds entries-written 1",
        ),
        (
            "zero-cpu",
            valid.replacen("#P:4", "#P:0", 1),
            "CPU count must be greater than zero",
        ),
        (
            "missing-legend",
            valid.replacen("# / _----=> need-resched\n", "", 1),
            "flag legend is out of order",
        ),
        (
            "missing-column",
            valid.replacen("  FUNCTION\n", "\n", 1),
            "column header lacks FUNCTION",
        ),
        (
            "event-count-mismatch",
            valid.replacen("1/1", "2/2", 1),
            "declares 2 buffered events, but text contains 1",
        ),
        (
            "cpu-out-of-range",
            valid.replacen("[002]", "[004]", 1),
            "CPU 4 is outside header CPU count 4",
        ),
        (
            "late-header",
            format!("{valid}# tracer: nop\n"),
            "ftrace header appears after events",
        ),
    ];
    let temp = tempfile::tempdir().unwrap();
    for (name, source, expected) in cases {
        let input = temp.path().join(format!("{name}.ftrace"));
        let output = temp.path().join(format!("{name}-parquet"));
        fs::write(&input, source).unwrap();
        let error = ftrace2parquet::convert(&input, &output, "boottime")
            .expect_err("malformed header must fail");
        let message = format!("{error:#}");
        assert!(
            message.contains(expected),
            "{name}: expected {expected:?} in {message:?}"
        );
        assert!(!output.exists(), "published {name}");
    }
}

#[test]
fn crosses_the_bounded_batch_without_losing_rows() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("large.ftrace");
    let output = temp.path().join("large");
    let mut source = String::new();
    for index in 0..8_193 {
        source.push_str(&format!(
            "worker-7 ( 7) [002] d.... {index}.0: sched_wakeup: comm=target pid=8 prio=120 target_cpu=003\n"
        ));
    }
    fs::write(&input, trace(&source, 8_193, 8_193, 4, true)).unwrap();
    ftrace2parquet::convert(&input, &output, "boottime").unwrap();
    let rows = ParquetRecordBatchReaderBuilder::try_new(
        File::open(output.join("text_ftrace_event.parquet")).unwrap(),
    )
    .unwrap()
    .build()
    .unwrap()
    .map(|batch| batch.unwrap().num_rows())
    .sum::<usize>();
    assert_eq!(rows, 8_193);
}

#[test]
fn invalid_input_and_existing_output_are_never_replaced() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("invalid.ftrace");
    let output = temp.path().join("parquet");
    fs::write(&input, trace("not an event\n", 1, 1, 4, true)).unwrap();
    assert!(ftrace2parquet::convert(&input, &output, "boottime").is_err());
    assert!(!output.exists());

    fs::write(&output, "sentinel").unwrap();
    assert!(ftrace2parquet::convert(&input, &output, "boottime").is_err());
    assert_eq!(fs::read_to_string(output).unwrap(), "sentinel");
}

#[test]
fn rejects_utf8_size_clock_and_empty_trace_boundaries_without_publication() {
    let temp = tempfile::tempdir().unwrap();
    let mut invalid_utf8 = header(1, 1, 4, true).into_bytes();
    invalid_utf8.extend_from_slice(&[0xff, b'\n']);
    let mut oversized = header(1, 1, 4, true).into_bytes();
    oversized.extend(vec![b'x'; 1024 * 1024 + 1]);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", header(0, 0, 4, true).into_bytes()),
        ("utf8", invalid_utf8),
        ("oversized", oversized),
        (
            "precision",
            trace(
                "worker-7 ( 7) [002] d.... 1.123456789001: event: payload\n",
                1,
                1,
                4,
                true,
            )
            .into_bytes(),
        ),
        (
            "overflow",
            trace(
                "worker-7 ( 7) [002] d.... 18446744074.0: event: payload\n",
                1,
                1,
                4,
                true,
            )
            .into_bytes(),
        ),
    ];
    for (name, source) in cases {
        let input = temp.path().join(format!("{name}.ftrace"));
        let output = temp.path().join(format!("{name}-parquet"));
        fs::write(&input, source).unwrap();
        assert!(
            ftrace2parquet::convert(&input, &output, "monotonic").is_err(),
            "accepted {name}"
        );
        assert!(!output.exists(), "published {name}");
    }
}

#[test]
fn cli_requires_the_explicit_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_ftrace2parquet"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("to a Parquet catalog directory"));
    for option in ["--input", "--output", "--clock-domain"] {
        assert!(stdout.contains(option), "missing {option} in {stdout}");
    }
}
