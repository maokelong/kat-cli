use std::{fs, fs::File};

use arrow_array::{Array, BooleanArray, Int32Array, StringArray, UInt32Array, UInt64Array};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

const MATERIALIZATION_VERSION_METADATA_KEY: &str = "kat.materialization.version";

fn assert_all_relations_have_materialization_version(
    root: &std::path::Path,
    expected_version: &str,
) {
    let mut relation_count = 0;
    for entry in fs::read_dir(root).expect("relation root can be listed") {
        let path = entry.expect("relation entry can be read").path();
        if path
            .extension()
            .is_none_or(|extension| extension != "parquet")
        {
            continue;
        }
        relation_count += 1;
        let builder = ParquetRecordBatchReaderBuilder::try_new(
            File::open(&path)
                .unwrap_or_else(|error| panic!("failed to open {}: {error}", path.display())),
        )
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        assert_eq!(
            builder
                .schema()
                .metadata()
                .get(MATERIALIZATION_VERSION_METADATA_KEY)
                .map(String::as_str),
            Some(expected_version),
            "relation {} has the wrong materialization version metadata",
            path.display()
        );
    }
    assert!(relation_count > 0, "expected at least one Parquet relation");
}

fn read_first_batch(path: &std::path::Path) -> arrow_array::RecordBatch {
    ParquetRecordBatchReaderBuilder::try_new(File::open(path).unwrap())
        .unwrap()
        .build()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
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

fn i32_value(batch: &arrow_array::RecordBatch, column: &str) -> i32 {
    batch
        .column_by_name(column)
        .unwrap()
        .as_any()
        .downcast_ref::<Int32Array>()
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

fn u64_value(batch: &arrow_array::RecordBatch, column: &str) -> u64 {
    batch
        .column_by_name(column)
        .unwrap()
        .as_any()
        .downcast_ref::<UInt64Array>()
        .unwrap()
        .value(0)
}

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

    let report = kat_datasource::decode_text_ftrace(&input, &output, "monotonic").unwrap();
    assert_eq!(report.unsupported_event_names(), &["custom_event"]);
    assert_all_relations_have_materialization_version(&output, "text-ftrace-v1");

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
    let unsupported = ParquetRecordBatchReaderBuilder::try_new(
        File::open(output.join("text_ftrace_unsupported_event.parquet")).unwrap(),
    )
    .unwrap()
    .build()
    .unwrap()
    .next()
    .unwrap()
    .unwrap();
    assert_eq!(
        unsupported
            .column_by_name("event_name")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "custom_event"
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
    for discarded_statistic in ["entries_in_buffer", "entries_written", "cpu_count"] {
        assert!(header.column_by_name(discarded_statistic).is_none());
    }
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
fn existing_empty_output_is_rejected_before_input_is_read() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("missing.ftrace");
    let output = temp.path().join("parquet");
    fs::create_dir(&output).unwrap();

    let error = kat_datasource::decode_text_ftrace(&input, &output, "monotonic")
        .expect_err("existing output must win over a missing input");

    assert!(
        error.to_string().contains("output already exists"),
        "unexpected error: {error:#}"
    );
    assert!(
        fs::read_dir(&output).unwrap().next().is_none(),
        "empty caller-owned output must remain untouched"
    );
}

#[test]
fn dangling_output_link_is_rejected_before_input_is_read() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("missing.ftrace");
    let missing_target = temp.path().join("missing-target");
    let output = temp.path().join("parquet");
    create_dangling_directory_link(&missing_target, &output);

    let error = kat_datasource::decode_text_ftrace(&input, &output, "monotonic")
        .expect_err("dangling destination entry must win over a missing input");

    assert!(
        error.to_string().contains("output already exists"),
        "unexpected error: {error:#}"
    );
    assert!(is_link(&fs::symlink_metadata(&output).unwrap()));
}

#[cfg(unix)]
fn create_dangling_directory_link(target: &std::path::Path, link: &std::path::Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn create_dangling_directory_link(target: &std::path::Path, link: &std::path::Path) {
    match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => {}
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314) =>
        {
            fs::create_dir(target).unwrap();
            let output = std::process::Command::new("cmd")
                .args(["/d", "/c", "mklink", "/j"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "failed to create junction\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            fs::remove_dir(target).unwrap();
        }
        Err(error) => panic!("failed to create dangling directory symlink: {error}"),
    }
}

#[cfg(unix)]
fn is_link(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_link(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & 0x400 != 0
}

#[test]
fn ignores_optional_buffer_statistics_and_uses_the_event_column_contract() {
    let event =
        "worker-7 ( 7) [999] d.... 1.0: sched_wakeup: comm=target pid=8 prio=120 target_cpu=003\n";
    let numeric = "# entries-in-buffer/entries-written: 1/0   #P:0\n";
    let source = trace(event, 1, 0, 0, true);
    let variants = [
        (
            "placeholder",
            source.replace(
                numeric,
                "# entries-in-buffer/entries-written: %lu/%lu   #P:%d\n",
            ),
        ),
        (
            "unstructured",
            source.replace(
                numeric,
                "# entries-in-buffer/entries-written: unavailable\n",
            ),
        ),
        ("absent", source.replace(numeric, "")),
    ];
    let temp = tempfile::tempdir().unwrap();

    for (name, source) in variants {
        let input = temp.path().join(format!("{name}.ftrace"));
        let output = temp.path().join(format!("{name}-parquet"));
        fs::write(&input, source).unwrap();

        kat_datasource::decode_text_ftrace(&input, &output, "boottime").unwrap();

        let root = ParquetRecordBatchReaderBuilder::try_new(
            File::open(output.join("text_ftrace_event.parquet")).unwrap(),
        )
        .unwrap()
        .build()
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
        assert_eq!(
            root.column_by_name("cpu")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt32Array>()
                .unwrap()
                .value(0),
            999
        );
    }
}

#[test]
fn accepts_only_unknown_events_and_reports_sorted_unique_names() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("unknown.ftrace");
    let output = temp.path().join("parquet");
    fs::write(
        &input,
        trace(
            concat!(
                "worker-7 ( 7) [002] d.... 1.0: z_event: value=1\n",
                "worker-7 ( 7) [002] d.... 2.0: a_event: value=2\n",
                "worker-7 ( 7) [002] d.... 3.0: z_event: value=3\n",
            ),
            3,
            3,
            4,
            true,
        ),
    )
    .unwrap();

    let report = kat_datasource::decode_text_ftrace(&input, &output, "boottime").unwrap();

    assert_eq!(report.unsupported_event_names(), &["a_event", "z_event"]);
    assert!(output.join("text_ftrace_header.parquet").is_file());
    assert!(
        output
            .join("text_ftrace_unsupported_event.parquet")
            .is_file()
    );
    assert!(!output.join("text_ftrace_event.parquet").exists());
    assert!(!output.join("text_ftrace_event_occurrence.parquet").exists());
}

#[test]
fn creates_payload_tables_only_when_observed() {
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
    kat_datasource::decode_text_ftrace(&input, &output, "boottime").unwrap();
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
fn parses_sched_filemap_block_binder_and_print_events() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("extended.ftrace");
    let output = temp.path().join("parquet");
    fs::write(
        &input,
        trace(
            concat!(
                "worker-7 ( 7) [002] d.... 1.0: sched_blocked_reason: pid=13 iowait=1 caller=io_schedule+0x4c/0x80\n",
                "worker-7 ( 7) [002] d.... 2.0: mm_filemap_add_to_page_cache: dev 179:2 ino 1a2b pfn=0x123 ofs=4096 order=0\n",
                "worker-7 ( 7) [002] d.... 3.0: mm_filemap_delete_from_page_cache: dev 253:6 ino 9a64 page=000000006e0f8322 pfn=797894 ofs=8192\n",
                "worker-7 ( 7) [002] d.... 4.0: block_rq_issue: 179,0 WS 4096 () 123 + 8 [kworker/u16:3]\n",
                "worker-7 ( 7) [002] d.... 5.0: block_rq_complete: 179,0 WS () 123 + 8 [-5]\n",
                "worker-7 ( 7) [002] d.... 6.0: binder_transaction: transaction=515671 dest_node=0 dest_proc=12974 dest_thread=12974 reply=1 flags=0x10 code=0x83\n",
                "worker-7 ( 7) [002] d.... 7.0: print: trace_printk: message: detail\n",
            ),
            7,
            7,
            4,
            true,
        ),
    )
    .unwrap();

    let report = kat_datasource::decode_text_ftrace(&input, &output, "boottime").unwrap();
    assert!(report.unsupported_event_names().is_empty());
    assert_eq!(row_count(&output.join("text_ftrace_event.parquet")), 7);

    let sched = read_first_batch(&output.join("text_ftrace_event_sched_blocked_reason.parquet"));
    assert_eq!(i32_value(&sched, "pid"), 13);
    assert_eq!(u32_value(&sched, "io_wait"), 1);
    assert_eq!(string_value(&sched, "caller"), "io_schedule+0x4c/0x80");

    let add =
        read_first_batch(&output.join("text_ftrace_event_mm_filemap_add_to_page_cache.parquet"));
    assert_eq!(u32_value(&add, "device_major"), 179);
    assert_eq!(u32_value(&add, "device_minor"), 2);
    assert_eq!(u64_value(&add, "inode"), 0x1a2b);
    assert_eq!(u64_value(&add, "page_frame_number"), 0x123);
    assert_eq!(u64_value(&add, "offset_bytes"), 4096);
    assert_eq!(u32_value(&add, "order"), 0);

    let delete = read_first_batch(
        &output.join("text_ftrace_event_mm_filemap_delete_from_page_cache.parquet"),
    );
    assert_eq!(u64_value(&delete, "page_frame_number"), 797_894);
    assert_eq!(string_value(&delete, "page_address"), "000000006e0f8322");
    assert!(delete.column_by_name("order").unwrap().is_null(0));

    let issue = read_first_batch(&output.join("text_ftrace_event_block_rq_issue.parquet"));
    assert_eq!(u32_value(&issue, "bytes"), 4096);
    assert_eq!(u64_value(&issue, "sector"), 123);
    assert_eq!(u32_value(&issue, "sector_count"), 8);
    assert_eq!(string_value(&issue, "process_name"), "kworker/u16:3");

    let complete = read_first_batch(&output.join("text_ftrace_event_block_rq_complete.parquet"));
    assert_eq!(i32_value(&complete, "error"), -5);

    let binder = read_first_batch(&output.join("text_ftrace_event_binder_transaction.parquet"));
    assert_eq!(i32_value(&binder, "transaction_id"), 515_671);
    assert_eq!(i32_value(&binder, "destination_process_id"), 12_974);
    assert_eq!(u32_value(&binder, "flags"), 0x10);
    assert_eq!(u32_value(&binder, "code"), 0x83);

    let print = read_first_batch(&output.join("text_ftrace_event_print.parquet"));
    assert_eq!(string_value(&print, "instruction_pointer"), "trace_printk");
    assert_eq!(string_value(&print, "content"), "message: detail");
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

    kat_datasource::decode_text_ftrace(&input, &output, "boottime").unwrap();

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
fn rejects_malformed_structural_headers_without_publication() {
    let event =
        "worker-7 ( 7) [002] d.... 1.0: sched_wakeup: comm=target pid=8 prio=120 target_cpu=003\n";
    let valid = trace(event, 1, 1, 4, true);
    let cases = [
        (
            "missing-tracer",
            valid.replacen("# tracer: nop\n", "", 1),
            "flag legend precedes tracer",
        ),
        (
            "duplicate-tracer",
            valid.replacen("# tracer: nop\n", "# tracer: nop\n# tracer: nop\n", 1),
            "duplicate tracer header",
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
        let error = kat_datasource::decode_text_ftrace(&input, &output, "boottime")
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
    kat_datasource::decode_text_ftrace(&input, &output, "boottime").unwrap();
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
    assert!(kat_datasource::decode_text_ftrace(&input, &output, "boottime").is_err());
    assert!(!output.exists());

    fs::write(&output, "sentinel").unwrap();
    assert!(kat_datasource::decode_text_ftrace(&input, &output, "boottime").is_err());
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
            kat_datasource::decode_text_ftrace(&input, &output, "monotonic").is_err(),
            "accepted {name}"
        );
        assert!(!output.exists(), "published {name}");
    }
}
