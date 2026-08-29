use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use prost::Message;
use rusqlite::Connection;

#[allow(dead_code)]
mod support;
use support::cargo_kat;

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
        per_cpu: vec![PerCpu { cpu: 0 }],
        trace_clock: "boot".to_owned(),
    };
    let result = TraceResult {
        stats: vec![stats(0), stats(1)],
        details: vec![Detail {
            cpu: 0,
            events: vec![Event {
                timestamp: 42,
                switch: Some(Switch {
                    previous_name: "idle".to_owned(),
                    previous_id: 0,
                    next_name: "render".to_owned(),
                    next_id: 7,
                }),
            }],
        }],
    };
    let frame = Envelope {
        name: "ftrace-plugin".to_owned(),
        data: result.encode_to_vec(),
    }
    .encode_to_vec();
    fs::write(path, profiler_section(0, &[frame])).unwrap();
}

fn profiler_section(data_type: u32, frames: &[Vec<u8>]) -> Vec<u8> {
    let body_length = frames.iter().map(|frame| 4 + frame.len()).sum::<usize>();
    let mut bytes = vec![0; HEADER_SIZE];
    bytes[0..8].copy_from_slice(&HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((HEADER_SIZE + body_length) as u64).to_le_bytes());
    bytes[56..60].copy_from_slice(&data_type.to_le_bytes());
    for frame in frames {
        bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        bytes.extend_from_slice(frame);
    }
    bytes
}

fn stage_skill(root: &Path) -> PathBuf {
    support::stage_skill(root, "skill").1
}

fn command(binary: &Path, root: &Path) -> Command {
    #[cfg(not(windows))]
    let command = {
        let mut command = Command::new(binary);
        command
            .env_remove("KAT_DATA_HOME")
            .env("XDG_DATA_HOME", root.join("xdg-data"))
            .env("HOME", root.join("home"));
        command
    };
    #[cfg(windows)]
    let command = {
        let _ = root;
        let mut command = Command::new(binary);
        command.env_remove("KAT_DATA_HOME");
        command
    };
    command
}

fn data_home(root: &Path) -> PathBuf {
    if cfg!(windows) {
        directories::ProjectDirs::from("", "", "KAT")
            .expect("Windows runner has a standard user data directory")
            .data_dir()
            .to_path_buf()
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
        vec!["a_table", "z_table"]
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
        [
            "clock_domain",
            "clock_snapshot",
            "profiler_payload_occurrence",
            "protobuf_enum_symbol",
            "trace_plugin_result",
            "trace_plugin_result_ftrace_cpu_detail",
            "trace_plugin_result_ftrace_cpu_detail_event",
            "trace_plugin_result_ftrace_cpu_detail_event_sched_switch_format",
            "trace_plugin_result_ftrace_cpu_stats",
            "trace_plugin_result_ftrace_cpu_stats_per_cpu_stats",
        ]
    );
}

#[test]
fn hitrace_import_reports_sorted_unknown_plugins_and_sections() {
    let temp = tempfile::tempdir().unwrap();
    let binary = stage_skill(temp.path());
    let source = temp.path().join("capture.htrace");
    let dataset = temp.path().join("dataset");
    let frames = [
        Envelope {
            name: "z-plugin".to_owned(),
            data: vec![1],
        }
        .encode_to_vec(),
        Envelope {
            name: "a-plugin_config".to_owned(),
            data: vec![2],
        }
        .encode_to_vec(),
        Envelope {
            name: "z-plugin".to_owned(),
            data: vec![3],
        }
        .encode_to_vec(),
    ];
    let mut bytes = profiler_section(0, &frames);
    bytes.extend(profiler_section(1000, &[]));
    bytes.extend(profiler_section(77, &[]));
    fs::write(&source, bytes).unwrap();

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
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response["result"]["unsupported_plugins"],
        serde_json::json!(["a-plugin", "z-plugin"])
    );
    assert_eq!(
        response["result"]["unsupported_section_types"],
        serde_json::json!([77, 1000])
    );
}

#[test]
fn invalid_hitrace_does_not_mutate_authorized_overwrite_target() {
    let temp = tempfile::tempdir().unwrap();
    let binary = stage_skill(temp.path());
    let source = temp.path().join("invalid.htrace");
    let dataset = temp.path().join("dataset");
    fs::write(&source, b"not a Hitrace capture").unwrap();
    fs::create_dir(&dataset).unwrap();
    fs::write(dataset.join("sentinel"), "unchanged").unwrap();

    let output = command(&binary, temp.path())
        .args(["import", "hitrace", "--trace"])
        .arg(&source)
        .arg("--dataset")
        .arg(&dataset)
        .arg("--overwrite-dataset")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        fs::read_to_string(dataset.join("sentinel")).unwrap(),
        "unchanged"
    );
}

#[cfg(unix)]
#[test]
fn hitrace_overwrite_rejects_target_that_contains_current_operation_log() {
    let temp = tempfile::tempdir().unwrap();
    let binary = stage_skill(temp.path());
    let source = temp.path().join("capture.htrace");
    let dataset = data_home(temp.path());
    hitrace(&source);
    fs::create_dir_all(&dataset).unwrap();
    fs::write(dataset.join(".kat-dataset"), "").unwrap();
    fs::write(dataset.join("sentinel"), "unchanged").unwrap();

    let output = command(&binary, temp.path())
        .args(["import", "hitrace", "--trace"])
        .arg(&source)
        .arg("--dataset")
        .arg(&dataset)
        .arg("--overwrite-dataset")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    let log = PathBuf::from(response["log_path"].as_str().unwrap());
    assert!(log.starts_with(dataset.join("logs")));
    assert!(log.is_file());
    assert!(
        fs::read_to_string(&log)
            .unwrap()
            .contains("status: failure")
    );
    assert!(dataset.join(".kat-dataset").is_file());
    assert_eq!(
        fs::read_to_string(dataset.join("sentinel")).unwrap(),
        "unchanged"
    );
}

#[test]
fn failed_hitrace_import_logs_unknown_content_observed_before_the_error() {
    let temp = tempfile::tempdir().unwrap();
    let binary = stage_skill(temp.path());
    let source = temp.path().join("partially-invalid.htrace");
    let dataset = temp.path().join("dataset");
    let unknown = Envelope {
        name: "future-plugin".to_owned(),
        data: vec![1],
    }
    .encode_to_vec();
    let mut bytes = profiler_section(0, &[unknown]);
    bytes.extend_from_slice(b"truncated-section");
    fs::write(&source, bytes).unwrap();

    let output = command(&binary, temp.path())
        .args(["import", "hitrace", "--trace"])
        .arg(&source)
        .arg("--dataset")
        .arg(&dataset)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert!(log.contains("unsupported plugin \"future-plugin\" at byte "));
    assert!(log.contains("status: failure"));
    assert!(!dataset.join(".kat-dataset").is_file());
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires an isolated Windows user profile; full-ci runs it on windows-latest"
)]
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
