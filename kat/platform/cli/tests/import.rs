use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use prost::Message;
use rusqlite::Connection;

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
}

#[derive(Clone, PartialEq, Message)]
struct Stats {
    #[prost(int32, tag = "1")]
    status: i32,
    #[prost(message, repeated, tag = "2")]
    per_cpu: Vec<PerCpu>,
    #[prost(string, tag = "3")]
    trace_clock: String,
}

#[derive(Clone, PartialEq, Message)]
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

fn hitrace(path: &Path) {
    let stats = |status| Stats {
        status,
        per_cpu: vec![PerCpu { cpu: 0 }, PerCpu { cpu: 1 }],
        trace_clock: "boot".to_owned(),
    };
    let result = TraceResult {
        stats: vec![stats(0), stats(1)],
        details: vec![
            Detail {
                cpu: 0,
                events: vec![
                    switch_event(100, 0, "idle", 7, "render"),
                    switch_event(150, 7, "render", 8, "worker"),
                    switch_event(175, 8, "worker", 0, "idle"),
                    switch_event(180, 0, "idle", 9, "tail"),
                ],
            },
            Detail {
                cpu: 1,
                events: vec![
                    switch_event(100, 0, "idle", 7, "render"),
                    switch_event(130, 7, "render", 0, "idle"),
                ],
            },
        ],
    };
    let frame = Envelope {
        name: "ftrace-plugin".to_owned(),
        data: result.encode_to_vec(),
    }
    .encode_to_vec();
    let mut bytes = vec![0; HEADER_SIZE];
    bytes[0..8].copy_from_slice(&HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((HEADER_SIZE + 4 + frame.len()) as u64).to_le_bytes());
    bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&frame);
    fs::write(path, bytes).unwrap();
}

fn switch_event(
    timestamp: u64,
    previous_id: i32,
    previous_name: &str,
    next_id: i32,
    next_name: &str,
) -> Event {
    Event {
        timestamp,
        switch: Some(Switch {
            previous_name: previous_name.to_owned(),
            previous_id,
            next_name: next_name.to_owned(),
            next_id,
        }),
    }
}

fn cargo_kat() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kat"))
}

fn stage_skill(root: &Path) -> PathBuf {
    let skill = root.join("skill");
    let target = if cfg!(windows) {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    };
    let binary_name = if cfg!(windows) { "kat.exe" } else { "kat" };
    let payload = skill.join("scripts").join("targets").join(target);
    fs::create_dir_all(&payload).unwrap();
    fs::write(skill.join("SKILL.md"), "# KAT\n").unwrap();
    let binary = payload.join(binary_name);
    fs::copy(cargo_kat(), &binary).unwrap();
    binary
}

fn command(binary: &Path, root: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("HOME", root.join("home"))
        .env("APPDATA", root.join("app-data"))
        .env("LOCALAPPDATA", root.join("local-app-data"))
        .env("USERPROFILE", root.join("profile"));
    command
}

fn data_home(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("app-data").join("KAT").join("data")
    } else {
        root.join("xdg-data").join("kat")
    }
}

fn database(path: &Path) {
    Connection::open(path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE z_table (ratio REAL); INSERT INTO z_table VALUES (2.5); \
             CREATE TABLE a_table (id INTEGER, label TEXT); INSERT INTO a_table VALUES (7, 'render'); \
             CREATE VIEW a_view (id, label) AS SELECT id, label FROM a_table;",
        )
        .unwrap();
}

#[test]
fn trace_streamer_import_then_inspect_is_a_real_json_process_loop() {
    let temp = tempfile::tempdir().unwrap();
    let binary = stage_skill(temp.path());
    let cwd = temp.path().join("cwd");
    fs::create_dir(&cwd).unwrap();
    let source = cwd.join("source.db");
    database(&source);
    let dataset = cwd.join("数据集");

    let imported = command(&binary, temp.path())
        .current_dir(&cwd)
        .args([
            "import",
            "trace-streamer",
            "--database",
            "source.db",
            "--dataset",
            "数据集",
        ])
        .output()
        .unwrap();

    assert_eq!(
        imported.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(imported.stderr.is_empty());
    let response: serde_json::Value = serde_json::from_slice(&imported.stdout).unwrap();
    assert_eq!(response["status"], "success");
    assert_eq!(
        response["result"],
        serde_json::json!({"path": dunce::canonicalize(&dataset).unwrap().to_str().unwrap()})
    );
    assert!(response.get("log_path").is_none());

    let inspected = command(&binary, temp.path())
        .current_dir(&cwd)
        .args(["inspect", "--dataset", "数据集"])
        .output()
        .unwrap();
    assert_eq!(
        inspected.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let inspection: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(
        inspection["result"]["tables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|table| table["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["a_table", "a_view", "z_table"]
    );
}

#[test]
fn hitrace_import_publishes_long_term_tables_result_and_operation_log() {
    let temp = tempfile::tempdir().unwrap();
    let binary = stage_skill(temp.path());
    let source = temp.path().join("capture.htrace");
    let dataset = temp.path().join("dataset");
    hitrace(&source);

    let output = command(&binary, temp.path())
        .args(["import", "hitrace", "--trace"])
        .arg(&source)
        .arg("--dataset")
        .arg(&dataset)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "success");
    assert_eq!(
        response["result"]["unsupported_plugins"],
        serde_json::json!([])
    );
    assert_eq!(
        response["result"]["unsupported_section_types"],
        serde_json::json!([])
    );
    assert_eq!(
        response["result"]["path"],
        dunce::canonicalize(&dataset).unwrap().to_str().unwrap()
    );
    let log = PathBuf::from(response["log_path"].as_str().unwrap());
    assert!(log.is_file());
    assert!(fs::read_to_string(log).unwrap().contains("status: success"));

    let inspected = command(&binary, temp.path())
        .arg("inspect")
        .arg("--dataset")
        .arg(&dataset)
        .output()
        .unwrap();
    let inspection: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(
        inspection["result"]["tables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|table| table["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["clock_domain", "clock_snapshot", "sched_switch"]
    );
}

#[test]
#[ignore = "requires KAT_E2E_SKILL_ROOT and KAT_REAL_HITRACE for the real capture loop"]
fn hitrace_to_kernel_pack_query_is_a_real_process_loop() {
    let skill = PathBuf::from(
        std::env::var_os("KAT_E2E_SKILL_ROOT")
            .expect("KAT_E2E_SKILL_ROOT must name the complete Skill deployment"),
    );
    let binary = if cfg!(windows) {
        skill.join("scripts/targets/windows-x86_64/kat.exe")
    } else {
        skill.join("scripts/targets/linux-x86_64/kat")
    };
    assert!(binary.is_file());
    assert!(skill.join("assets/packs/kat-kernel/pack.toml").is_file());
    let trace = PathBuf::from(
        std::env::var_os("KAT_REAL_HITRACE")
            .expect("KAT_REAL_HITRACE must name a real OpenHarmony zero-loss capture"),
    );
    assert!(trace.is_file());

    let temporary = tempfile::tempdir().unwrap();
    let dataset = temporary.path().join("dataset");

    let imported = command(&binary, temporary.path())
        .args(["import", "hitrace", "--trace"])
        .arg(&trace)
        .arg("--dataset")
        .arg(&dataset)
        .output()
        .unwrap();
    assert_eq!(
        imported.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );

    let inspected = command(&binary, temporary.path())
        .args(["inspect", "--pack", "kat-kernel"])
        .output()
        .unwrap();
    assert_eq!(inspected.status.code(), Some(0));
    let inspection: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inspection["result"]["name"], "kat-kernel");
    assert_eq!(
        inspection["result"]["workflows"],
        serde_json::json!([{
            "name": "thread-cpu-time",
            "title": "Thread CPU Time by CPU",
            "description": "Aggregate complete observed non-idle scheduling intervals by thread and CPU.",
            "required_tables": ["sched_switch"],
            "parameters": []
        }])
    );

    let tested = command(&binary, temporary.path())
        .args(["test", "--pack", "kat-kernel"])
        .output()
        .unwrap();
    assert_eq!(
        tested.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&tested.stderr)
    );

    let run = command(&binary, temporary.path())
        .args([
            "run",
            "--pack",
            "kat-kernel",
            "--workflow",
            "thread-cpu-time",
            "--dataset",
        ])
        .arg(&dataset)
        .output()
        .unwrap();
    assert_eq!(
        run.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let run_response: serde_json::Value = serde_json::from_slice(&run.stdout).unwrap();
    let run_id = run_response["result"]["run_id"].as_str().unwrap();
    assert!(
        run_response["result"]["outputs"]["thread_cpu_time_by_cpu"]["row_count"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        run_response["result"]["outputs"]["thread_cpu_time_by_cpu"]["columns"],
        serde_json::json!([
            {"name": "thread_id", "type": "int32"},
            {"name": "thread_name", "type": "string"},
            {"name": "cpu", "type": "uint32"},
            {"name": "observed_cpu_time_ns", "type": "int64"}
        ])
    );

    let totals = command(&binary, temporary.path())
        .args(["query", "--run", run_id, "--sql"])
        .arg(
            "SELECT thread_id, thread_name, SUM(observed_cpu_time_ns) AS total_cpu_time_ns \
             FROM output.thread_cpu_time_by_cpu GROUP BY thread_id, thread_name \
             ORDER BY total_cpu_time_ns DESC, thread_id, thread_name LIMIT 10",
        )
        .output()
        .unwrap();
    assert_eq!(totals.status.code(), Some(0));
    let totals: serde_json::Value = serde_json::from_slice(&totals.stdout).unwrap();
    let total_rows = totals["result"]["rows"].as_array().unwrap();
    assert!(!total_rows.is_empty());
    assert!(total_rows.len() <= 10);
    assert!(
        total_rows
            .iter()
            .all(|row| row.as_array().unwrap().len() == 3)
    );

    let cpus = command(&binary, temporary.path())
        .args(["query", "--run", run_id, "--sql"])
        .arg(
            "SELECT thread_id, thread_name, cpu, observed_cpu_time_ns \
             FROM output.thread_cpu_time_by_cpu \
             ORDER BY observed_cpu_time_ns DESC, thread_id, thread_name, cpu LIMIT 10",
        )
        .output()
        .unwrap();
    assert_eq!(cpus.status.code(), Some(0));
    let cpus: serde_json::Value = serde_json::from_slice(&cpus.stdout).unwrap();
    let cpu_rows = cpus["result"]["rows"].as_array().unwrap();
    assert!(!cpu_rows.is_empty());
    assert!(cpu_rows.len() <= 10);
    assert!(
        cpu_rows
            .iter()
            .all(|row| row.as_array().unwrap().len() == 4)
    );
}

#[test]
#[ignore = "requires KAT_E2E_SKILL_ROOT and KAT_REAL_TRACE_STREAMER_DB for the real Demo loop"]
fn trace_streamer_to_openharmony_demo_query_is_a_real_process_loop() {
    let skill = PathBuf::from(
        std::env::var_os("KAT_E2E_SKILL_ROOT")
            .expect("KAT_E2E_SKILL_ROOT must name the complete Skill deployment"),
    );
    let binary = if cfg!(windows) {
        skill.join("scripts/targets/windows-x86_64/kat.exe")
    } else {
        skill.join("scripts/targets/linux-x86_64/kat")
    };
    let database = PathBuf::from(std::env::var_os("KAT_REAL_TRACE_STREAMER_DB").expect(
        "KAT_REAL_TRACE_STREAMER_DB must name the normalized seven-table OpenHarmony fixture",
    ));
    assert!(binary.is_file());
    assert!(database.is_file());
    assert!(
        skill
            .join("assets/packs/kat-openharmony-demo/pack.toml")
            .is_file()
    );

    let temporary = tempfile::tempdir().unwrap();
    let dataset = temporary.path().join("dataset");
    let imported = command(&binary, temporary.path())
        .args(["import", "trace-streamer", "--database"])
        .arg(&database)
        .arg("--dataset")
        .arg(&dataset)
        .output()
        .unwrap();
    assert_eq!(
        imported.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );

    let inspected = command(&binary, temporary.path())
        .args(["inspect", "--pack", "kat-openharmony-demo"])
        .output()
        .unwrap();
    assert_eq!(inspected.status.code(), Some(0));
    let inspection: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(
        inspection["result"]["workflows"],
        serde_json::json!([{
            "name": "first-frame-scheduling-dependencies",
            "title": "First-frame Scheduling Dependencies",
            "description": "Analyze observable scheduling dependencies for the earliest completed actual frame.",
            "required_tables": [
                "args", "data_dict", "frame_slice", "instant", "process", "thread", "thread_state"
            ],
            "parameters": [{
                "name": "process_name",
                "option": "--process-name",
                "type": "string",
                "required": true,
                "description": "Exact process name to analyze."
            }]
        }])
    );

    let tested = command(&binary, temporary.path())
        .args(["test", "--pack", "kat-openharmony-demo"])
        .output()
        .unwrap();
    assert_eq!(
        tested.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&tested.stderr)
    );

    let run = command(&binary, temporary.path())
        .args([
            "run",
            "--pack",
            "kat-openharmony-demo",
            "--workflow",
            "first-frame-scheduling-dependencies",
            "--dataset",
        ])
        .arg(&dataset)
        .args(["--", "--process-name", ".tencent.wechat"])
        .output()
        .unwrap();
    assert_eq!(
        run.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let run_response: serde_json::Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(
        run_response["result"]["outputs"]["scheduling_dependencies"]["row_count"],
        11
    );
    let run_id = run_response["result"]["run_id"].as_str().unwrap();

    let query = command(&binary, temporary.path())
        .args(["query", "--run", run_id, "--sql"])
        .arg(
            "SELECT clock_value, duration_ns, frame_thread_state, blocker_thread_id, \
             blocker_thread_state, blocker_cpu FROM output.scheduling_dependencies \
             ORDER BY clock_value LIMIT 20",
        )
        .output()
        .unwrap();
    assert_eq!(query.status.code(), Some(0));
    let query: serde_json::Value = serde_json::from_slice(&query.stdout).unwrap();
    assert_eq!(
        query["result"]["rows"],
        serde_json::json!([
            ["246270250000", "127000", "Running", "15426", "Running", "0"],
            ["246270377000", "57000", "S", "2734", "Running", "5"],
            ["246270434000", "267000", "S", "1337", "Running", "8"],
            ["246270701000", "6000", "S", "1814", "R", null],
            ["246270707000", "169000", "S", "1814", "Running", "5"],
            ["246270876000", "13000", "S", "1337", "R", null],
            ["246270889000", "49000", "S", "1337", "Running", "8"],
            ["246270938000", "26000", "S", "2734", "R", null],
            ["246270964000", "35000", "S", "2734", "Running", "1"],
            ["246270999000", "35000", "R", "15426", "R", null],
            ["246271034000", "57000", "Running", "15426", "Running", "1"]
        ])
    );

    let runs = data_home(temporary.path()).join("runs");
    let published_run_count = || {
        fs::read_dir(&runs)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().join("manifest.json").is_file())
            .count()
    };
    let before_failures = published_run_count();
    for process_name in [".does-not-exist", "hitrace"] {
        let failed = command(&binary, temporary.path())
            .args([
                "run",
                "--pack",
                "kat-openharmony-demo",
                "--workflow",
                "first-frame-scheduling-dependencies",
                "--dataset",
            ])
            .arg(&dataset)
            .args(["--", "--process-name", process_name])
            .output()
            .unwrap();
        assert_eq!(failed.status.code(), Some(1));
        let response: serde_json::Value = serde_json::from_slice(&failed.stdout).unwrap();
        assert_eq!(response["status"], "failure");
        assert!(response.get("result").is_none());
    }
    assert_eq!(published_run_count(), before_failures);
}

#[test]
fn default_target_is_uuid_v7_under_data_home_and_is_inspectable() {
    let temp = tempfile::tempdir().unwrap();
    let binary = stage_skill(temp.path());
    let source = temp.path().join("source.db");
    database(&source);

    let output = command(&binary, temp.path())
        .args(["import", "trace-streamer", "--database"])
        .arg(&source)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let path = PathBuf::from(response["result"]["path"].as_str().unwrap());
    assert_eq!(
        path.parent().unwrap(),
        data_home(temp.path()).join("datasets")
    );
    let id = uuid::Uuid::parse_str(path.file_name().unwrap().to_str().unwrap()).unwrap();
    assert_eq!(id.get_version_num(), 7);
    assert!(path.join(".kat-dataset").is_file());
}

#[test]
fn overwrite_requires_explicit_target_and_replaces_every_entry() {
    let temp = tempfile::tempdir().unwrap();
    let binary = stage_skill(temp.path());
    let source = temp.path().join("source.db");
    database(&source);
    let target = temp.path().join("dataset");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep"), "old").unwrap();

    let refused = command(&binary, temp.path())
        .args(["import", "--dataset"])
        .arg(&target)
        .args(["trace-streamer", "--database"])
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(1));
    assert!(target.join("keep").exists());
    let failure: serde_json::Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(failure["status"], "failure");

    let replaced = command(&binary, temp.path())
        .args(["import", "trace-streamer", "--database"])
        .arg(&source)
        .args(["--dataset"])
        .arg(&target)
        .arg("--overwrite-dataset")
        .output()
        .unwrap();
    assert_eq!(
        replaced.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&replaced.stderr)
    );
    assert!(!target.join("keep").exists());

    let parse_failure = command(&binary, temp.path())
        .args(["import", "trace-streamer", "--database"])
        .arg(&source)
        .arg("--overwrite-dataset")
        .output()
        .unwrap();
    assert_eq!(parse_failure.status.code(), Some(2));
    assert!(parse_failure.stdout.is_empty());
}

#[test]
fn help_marks_trace_streamer_deprecated_and_explains_overwrite_risk() {
    for arguments in [
        &["import", "--help"][..],
        &["import", "trace-streamer", "--help"][..],
    ] {
        let help = Command::new(cargo_kat()).args(arguments).output().unwrap();
        assert_eq!(help.status.code(), Some(0));
        let help = String::from_utf8(help.stdout).unwrap();
        for text in [
            "Deprecated",
            "table interface is unstable",
            "removed before the first formal release",
        ] {
            assert!(help.contains(text), "missing {text:?}: {help}");
        }
        if arguments.len() > 2 {
            for text in [
                "--database",
                "--overwrite-dataset",
                "Permanently deletes all existing contents",
                "unrecognized files",
                "Linked or mounted paths",
                "No backup, rollback, or failure recovery",
            ] {
                assert!(help.contains(text), "missing {text:?}: {help}");
            }
        }
    }
}
