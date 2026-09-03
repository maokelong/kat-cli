use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[allow(dead_code)]
mod support;

#[allow(dead_code)]
#[path = "support/test_home.rs"]
mod test_home;

const PACK_NAME: &str = "kat-openharmony-critical-path";

#[test]
#[ignore = "requires KAT_TEST_PYTHON and a wheel built from the current checkout"]
fn critical_path_guides_drive_two_explicit_runs_through_public_operations() {
    let python = PathBuf::from(
        std::env::var_os("KAT_TEST_PYTHON").expect("KAT_TEST_PYTHON identifies CPython"),
    );
    let workflow_wheel = PathBuf::from(
        std::env::var_os("KAT_TEST_WORKFLOW_WHEEL")
            .expect("KAT_TEST_WORKFLOW_WHEEL identifies the current wheel"),
    );
    let temporary = tempfile::tempdir().expect("create chain test directory");
    let (_skill, binary) = support::stage_real_host_skill(
        temporary.path(),
        &support::cargo_kat(),
        &python,
        &workflow_wheel,
    );
    let pack = critical_path_pack();
    let sqlite = create_trace_streamer_fixture(&python, temporary.path());

    let locate_inspection = inspect_workflow(
        &binary,
        temporary.path(),
        &pack,
        "locate-first-actual-frame",
    );
    let locate_guide = locate_inspection["result"]["workflow"]["guide"]
        .as_str()
        .expect("locate Workflow publishes a Guide");
    for required_fact in [
        "frame_window",
        "root_itid",
        "start_ts",
        "end_ts",
        "extract-critical-path",
        "sqlite_path",
    ] {
        assert!(
            locate_guide.contains(required_fact),
            "locate Guide does not mention {required_fact}"
        );
    }

    let first_run = run_workflow(
        &binary,
        temporary.path(),
        &pack,
        "locate-first-actual-frame",
        &[
            "--sqlite-path",
            sqlite.to_str().unwrap(),
            "--process-name",
            ".demo",
        ],
    );
    let first_run_id = first_run["result"]["run_id"]
        .as_str()
        .expect("first Run has an identity");
    assert_eq!(
        output_names(&first_run),
        vec!["frame_window"],
        "first Run publishes its actual Output inventory"
    );
    assert_eq!(
        output_columns(&first_run, "frame_window"),
        vec![
            "frame_id",
            "root_itid",
            "start_ts",
            "end_ts",
            "duration_ns",
            "process_id",
            "process_name",
            "thread_id",
            "thread_name",
            "callstack_id",
            "clock_domain",
        ]
    );

    let window_query = query_run(
        &binary,
        temporary.path(),
        first_run_id,
        "SELECT root_itid, start_ts, end_ts FROM output.frame_window LIMIT 1",
    );
    let window_rows = query_rows(&window_query);
    assert_eq!(window_rows.len(), 1);
    let window = &window_rows[0];
    assert_eq!(window["root_itid"], 2);
    assert_eq!(window["start_ts"], 150);
    assert_eq!(window["end_ts"], 160);

    let extract_inspection =
        inspect_workflow(&binary, temporary.path(), &pack, "extract-critical-path");
    let extract_detail = &extract_inspection["result"]["workflow"];
    let extract_guide = extract_detail["guide"]
        .as_str()
        .expect("extract Workflow publishes a Guide");
    for required_fact in [
        "critical_path_segments",
        "critical_path_callstack_evidence",
        "segment_id",
        "uncertainty_reason",
    ] {
        assert!(
            extract_guide.contains(required_fact),
            "extract Guide does not mention {required_fact}"
        );
    }
    assert_eq!(
        parameter_names(extract_detail),
        vec![
            "sqlite_path",
            "root_itid",
            "start_ts",
            "end_ts",
            "max_depth",
            "min_segment_ms",
        ]
    );

    let root_itid = window["root_itid"].as_i64().unwrap().to_string();
    let start_ts = window["start_ts"].as_i64().unwrap().to_string();
    let end_ts = window["end_ts"].as_i64().unwrap().to_string();
    let second_run = run_workflow(
        &binary,
        temporary.path(),
        &pack,
        "extract-critical-path",
        &[
            "--sqlite-path",
            sqlite.to_str().unwrap(),
            "--root-itid",
            &root_itid,
            "--start-ts",
            &start_ts,
            "--end-ts",
            &end_ts,
        ],
    );
    let second_run_id = second_run["result"]["run_id"]
        .as_str()
        .expect("second Run has an identity");
    assert_ne!(first_run_id, second_run_id);
    assert_eq!(
        output_names(&second_run),
        vec!["critical_path_callstack_evidence", "critical_path_segments",]
    );

    let segments_query = query_run(
        &binary,
        temporary.path(),
        second_run_id,
        "SELECT segment_id, parent_segment_id, depth, duration_ns, thread_name, \
         segment_kind, relation_to_parent, termination_reason, uncertainty_reason \
         FROM output.critical_path_segments ORDER BY segment_id LIMIT 20",
    );
    let segment_rows = query_rows(&segments_query);
    assert_eq!(segment_rows.len(), 1);
    assert_eq!(segment_rows[0]["segment_id"], 0);
    assert_eq!(segment_rows[0]["duration_ns"], 10);
    assert_eq!(segment_rows[0]["thread_name"], "render");
    assert_eq!(segment_rows[0]["segment_kind"], "execution");
    assert_eq!(segment_rows[0]["relation_to_parent"], "root");

    let callstack_query = query_run(
        &binary,
        temporary.path(),
        second_run_id,
        "SELECT segment_id, function_name, business_category \
         FROM output.critical_path_callstack_evidence WHERE segment_id = 0 \
         ORDER BY start_ts, callstack_depth, callstack_id LIMIT 20",
    );
    assert_eq!(
        query_rows(&callstack_query),
        vec![serde_json::json!({
            "segment_id": 0,
            "function_name": "RenderFrame",
            "business_category": "application",
        })]
    );
}

fn critical_path_pack() -> PathBuf {
    dunce::canonicalize(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("packs")
            .join(PACK_NAME),
    )
    .expect("locate bundled critical-path PACK")
}

fn create_trace_streamer_fixture(python: &Path, root: &Path) -> PathBuf {
    let script = root.join("create-guide-chain-fixture.py");
    let database = root.join("guide-chain.db");
    fs::write(
        &script,
        r#"import sqlite3
import sys

with sqlite3.connect(sys.argv[1]) as connection:
    connection.executescript("""
        CREATE TABLE process(ipid INT, pid INT, name TEXT);
        CREATE TABLE frame_slice(
            id INT, itid INT, ts INT, dur INT, callstack_id INT, ipid INT, type INT
        );
        CREATE TABLE thread(itid INT, ipid INT, tid INT, name TEXT);
        CREATE TABLE thread_state(
            itid INT, ts INT, dur INT, state TEXT, cpu INT, arg_setid INT
        );
        CREATE TABLE sched_slice(ts INT, dur INT, cpu INT, priority INT, itid INT);
        CREATE TABLE callstack(
            id INT, parent_id INT, depth INT, ts INT, dur INT, name TEXT, callid INT
        );
        CREATE TABLE instant(
            wakeup_from INT, ref_type TEXT, ref INT, name TEXT, ts INT
        );
        CREATE TABLE args(argset INT, key INT, value INT, datatype INT);
        CREATE TABLE data_dict(id INT, data TEXT);

        INSERT INTO process VALUES (10, 1000, '.demo');
        INSERT INTO frame_slice VALUES (1, 1, 100, 100, NULL, 10, 0);
        INSERT INTO frame_slice VALUES (2, 2, 150, 10, 7, 10, 0);
        INSERT INTO thread VALUES (1, 10, 11, 'ui');
        INSERT INTO thread VALUES (2, 10, 22, 'render');
        INSERT INTO thread_state VALUES (2, 150, 10, 'Running', 3, NULL);
        INSERT INTO sched_slice VALUES (150, 10, 3, 120, 2);
        INSERT INTO callstack VALUES (7, NULL, 0, 150, 10, 'RenderFrame', 2);
    """)
"#,
    )
    .expect("write SQLite fixture builder");
    let output = Command::new(python)
        .arg(&script)
        .arg(&database)
        .output()
        .expect("create Trace Streamer SQLite fixture");
    assert!(
        output.status.success(),
        "fixture creation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    dunce::canonicalize(database).expect("canonicalize Trace Streamer SQLite fixture")
}

fn inspect_workflow(binary: &Path, root: &Path, pack: &Path, workflow: &str) -> serde_json::Value {
    let mut command = kat_command(binary, root);
    command
        .args(["inspect", "workflow", "--pack", PACK_NAME, "--workflow"])
        .arg(workflow)
        .arg("--pack-dir")
        .arg(pack);
    successful_response(command, "inspect Workflow")
}

fn run_workflow(
    binary: &Path,
    root: &Path,
    pack: &Path,
    workflow: &str,
    arguments: &[&str],
) -> serde_json::Value {
    let mut command = kat_command(binary, root);
    command
        .args(["run", "--pack", PACK_NAME, "--workflow"])
        .arg(workflow)
        .arg("--pack-dir")
        .arg(pack)
        .arg("--")
        .args(arguments);
    successful_response(command, "run Workflow")
}

fn query_run(binary: &Path, root: &Path, run_id: &str, sql: &str) -> serde_json::Value {
    let mut command = kat_command(binary, root);
    command.args(["query", "--run", run_id, "--sql", sql]);
    successful_response(command, "query Run Output")
}

fn kat_command(binary: &Path, root: &Path) -> Command {
    let mut command = Command::new(binary);
    test_home::configure(&mut command, root);
    command
}

fn successful_response(mut command: Command, operation: &str) -> serde_json::Value {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{operation}: {error}"));
    assert_eq!(
        output.status.code(),
        Some(0),
        "{operation} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "{operation} wrote to stderr");
    let response: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("KAT Response is JSON");
    assert_eq!(response["status"], "success", "{operation}: {response}");
    response
}

fn output_names(response: &serde_json::Value) -> Vec<&str> {
    response["result"]["outputs"]
        .as_object()
        .expect("Run publishes Output inventory")
        .keys()
        .map(String::as_str)
        .collect()
}

fn output_columns<'a>(response: &'a serde_json::Value, name: &str) -> Vec<&'a str> {
    response["result"]["outputs"][name]["columns"]
        .as_array()
        .expect("Output publishes columns")
        .iter()
        .map(|column| column["name"].as_str().expect("column name is text"))
        .collect()
}

fn parameter_names(workflow: &serde_json::Value) -> Vec<&str> {
    workflow["parameters"]
        .as_array()
        .expect("Workflow publishes parameters")
        .iter()
        .map(|parameter| parameter["name"].as_str().expect("parameter name is text"))
        .collect()
}

fn query_rows(response: &serde_json::Value) -> Vec<serde_json::Value> {
    let path = response["result"]["path"]
        .as_str()
        .expect("query Response publishes result.path");
    fs::read_to_string(path)
        .expect("read query NDJSON")
        .lines()
        .map(|line| serde_json::from_str(line).expect("query row is JSON"))
        .collect()
}
