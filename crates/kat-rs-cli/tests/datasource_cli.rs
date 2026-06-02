use assert_cmd::Command;
use predicates::str::contains;
use std::path::PathBuf;

const BYTRACE: &str = "tests/fixtures/traces/ut_bytrace_input_full.txt";
const BYTRACE_THREAD: &str = "tests/fixtures/traces/ut_bytrace_input_thread.txt";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn kat_rs() -> Command {
    let mut cmd = Command::cargo_bin("kat-rs").unwrap();
    cmd.current_dir(repo_root());
    cmd
}

#[test]
fn inspect_reports_sched_slice_rows() {
    let mut cmd = kat_rs();
    cmd.args(["datasource", "inspect", "--trace", BYTRACE, "--json"])
        .assert()
        .success()
        .stdout(contains("\"sched_slice\""))
        .stdout(contains("\"row_count\": 16"));
}

#[test]
fn query_reports_sched_slice_count() {
    let mut cmd = kat_rs();
    cmd.args([
        "datasource",
        "query",
        "--trace",
        BYTRACE,
        "--sql",
        "SELECT COUNT(*) AS slices FROM sched_slice",
        "--json",
    ])
    .assert()
    .success()
    .stdout(contains("\"slices\": 16"));
}

#[test]
fn query_accepts_repeated_trace_flags() {
    let mut cmd = kat_rs();
    cmd.args([
        "datasource",
        "query",
        "--trace",
        BYTRACE,
        "--trace",
        BYTRACE_THREAD,
        "--sql",
        "SELECT source_id, COUNT(*) AS slices FROM sched_slice GROUP BY source_id ORDER BY source_id",
        "--json",
    ])
    .assert()
    .success()
    .stdout(contains("\"source_id\": \"source_0\""))
    .stdout(contains("\"source_id\": \"source_1\""))
    .stdout(contains("\"slices\": 16"))
    .stdout(contains("\"slices\": 15"));
}

#[test]
fn query_supports_artifact_output_mode() {
    let mut cmd = kat_rs();
    cmd.args([
        "datasource",
        "query",
        "--trace",
        BYTRACE,
        "--sql",
        "SELECT * FROM raw_event",
        "--output",
        "artifact",
        "--json",
    ])
    .assert()
    .success()
    .stdout(contains("\"artifacts\""))
    .stdout(contains("\"format\": \"jsonl\""))
    .stdout(contains("\"rows\": []"));
}

#[test]
fn validate_reports_ok_for_bytrace_suite() {
    let mut cmd = kat_rs();
    cmd.args([
        "datasource",
        "validate",
        "--trace",
        BYTRACE,
        "--query-suite",
        "tests/golden/bytrace_full",
        "--json",
    ])
    .assert()
    .success()
    .stdout(contains("\"status\": \"ok\""));
}

#[test]
fn bench_reports_query_metrics() {
    let mut cmd = kat_rs();
    cmd.args([
        "datasource",
        "bench",
        "--trace",
        BYTRACE,
        "--sql",
        "SELECT COUNT(*) AS slices FROM sched_slice",
        "--json",
    ])
    .assert()
    .success()
    .stdout(contains("\"rows_returned\": 1"))
    .stdout(contains("\"open_dataset\""))
    .stdout(contains("\"session_build\""))
    .stdout(contains("\"cache_hit\": false"));
}
