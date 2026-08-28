use std::{fs, fs::File, process::Command};

use arrow_array::{Array, Int32Array, StringArray, UInt64Array};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

#[test]
fn converts_common_fields_unknown_events_and_exact_payload() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("trace.ftrace");
    let output = temp.path().join("trace.parquet");
    fs::write(
        &input,
        concat!(
            "# tracer: nop\n",
            " Render Thread (IO)-42 (-------) [007] dN..2 1.000000001: custom_event: value=1  \n",
            "worker-name-9       (    123) [002] d.... 1.5: sched_wakeup: comm=target pid=8\n",
        ),
    )
    .unwrap();

    ftrace2parquet::convert(&input, &output, "monotonic").unwrap();

    let batch = ParquetRecordBatchReaderBuilder::try_new(File::open(output).unwrap())
        .unwrap()
        .build()
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(batch.num_rows(), 2);
    assert_eq!(
        batch
            .column_by_name("source_event_sequence")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .values(),
        &[0, 1]
    );
    assert_eq!(
        batch
            .column_by_name("emitter_thread_name")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "Render Thread (IO)"
    );
    assert!(
        batch
            .column_by_name("emitter_process_id")
            .unwrap()
            .as_any()
            .downcast_ref::<Int32Array>()
            .unwrap()
            .is_null(0)
    );
    assert_eq!(
        batch
            .column_by_name("payload")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "value=1  "
    );
}

#[test]
fn crosses_the_bounded_batch_without_losing_rows() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("large.ftrace");
    let output = temp.path().join("large.parquet");
    let mut source = String::new();
    for index in 0..8_193 {
        source.push_str(&format!(
            "worker-7 ( 7) [002] d.... {index}.0: event: payload\n"
        ));
    }
    fs::write(&input, source).unwrap();
    ftrace2parquet::convert(&input, &output, "boottime").unwrap();
    let rows = ParquetRecordBatchReaderBuilder::try_new(File::open(output).unwrap())
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
    let output = temp.path().join("trace.parquet");
    fs::write(&input, "not an event\n").unwrap();
    assert!(ftrace2parquet::convert(&input, &output, "boottime").is_err());
    assert!(!output.exists());

    fs::write(&output, "sentinel").unwrap();
    assert!(ftrace2parquet::convert(&input, &output, "boottime").is_err());
    assert_eq!(fs::read_to_string(output).unwrap(), "sentinel");
}

#[test]
fn rejects_utf8_size_clock_and_empty_trace_boundaries_without_publication() {
    let temp = tempfile::tempdir().unwrap();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", b"# header only\n\n".to_vec()),
        ("utf8", vec![0xff, b'\n']),
        ("oversized", vec![b'x'; 1024 * 1024 + 1]),
        (
            "precision",
            b"worker-7 ( 7) [002] d.... 1.123456789001: event: payload\n".to_vec(),
        ),
        (
            "overflow",
            b"worker-7 ( 7) [002] d.... 18446744074.0: event: payload\n".to_vec(),
        ),
    ];
    for (name, source) in cases {
        let input = temp.path().join(format!("{name}.ftrace"));
        let output = temp.path().join(format!("{name}.parquet"));
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
    for option in ["--input", "--output", "--clock-domain"] {
        assert!(stdout.contains(option), "missing {option} in {stdout}");
    }
}
