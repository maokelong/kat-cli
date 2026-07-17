use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use base64::Engine;

const RUN_ID: &str = "019f6e00-0000-7000-8000-000000000031";
const OUTPUT_ID: &str = "0123456789abcdef0123456789abcdef";
const PARQUET: &str = "UEFSMRUEFSAVIEwVBBUAEgAAAQAAAAAAAAACAAAAAAAAABUAFRIVEiwVBBUQFQYVBgAAAgAAAAQBAQMCFQQVMBUwTBUEFQASAAAGAAAAY2FsbGVyCgAAAGZ1dGV4X3dhaXQVABUSFRIsFQQVEBUGFQYAAAIAAAAEAQEDAhkSAhkYCAEAAAAAAAAAGRgIAgAAAAAAAAAVAhkWACkmAAQAGRICGRgGY2FsbGVyGRgKZnV0ZXhfd2FpdBUCGRYAKSYABAAZHBZEFTQWAAAAGRwWxAEVNBYAABkWIAAVAhk8SAxhcnJvd19zY2hlbWEVBAAVBCUCGAJpZAAVDCUCGARkYXRhJQBMHAAAABYEGRwZLCYAHBUEGTUABhAZGAJpZBUAFgQWcBZwJkQmCBwYCAIAAAAAAAAAGAgBAAAAAAAAABYAKAgCAAAAAAAAABgIAQAAAAAAAAAREQAZLBUEFQAVAgAVABUQFQIAPDkmAAQAABaEAxUUFvgBFUYAJgAcFQwZNQAGEBkYBGRhdGEVABYEFoABFoABJsQBJngcNgAoCmZ1dGV4X3dhaXQYBmNhbGxlchERABksFQQVABUCABUAFRAVAgA8FiApJgAEAAAWmAMVHBa+AhVGABbwARYEJggW8AEUAAAZHBgMQVJST1c6c2NoZW1hGOwBLy8vLy82Z0FBQUFRQUFBQUFBQUtBQXdBQ2dBSkFBUUFDZ0FBQUJBQUFBQUFBUVFBQ0FBSUFBQUFCQUFJQUFBQUJBQUFBQUlBQUFCRUFBQUFCQUFBQU5ULy8vOFlBQUFBREFBQUFBQUFBUVVRQUFBQUFBQUFBQVFBQkFBRUFBQUFCQUFBQUdSaGRHRUFBQUFBRUFBVUFCQUFEZ0FQQUFRQUFBQUlBQkFBQUFBWUFBQUFJQUFBQUFBQUFRSWNBQUFBQ0FBTUFBUUFDd0FJQUFBQVFBQUFBQUFBQUFFQUFBQUFBZ0FBQUdsa0FBQT0AGBlwYXJxdWV0LXJzIHZlcnNpb24gNTguMy4wGSwcAAAcAAAALwIAAFBBUjE=";

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
    stage_fake_host(&binary);
    binary
}

fn stage_fake_host(binary: &Path) {
    let payload = binary.parent().unwrap();
    let host = if cfg!(windows) {
        payload.join("python/python.exe")
    } else {
        payload.join("python/bin/python3")
    };
    fs::create_dir_all(host.parent().unwrap()).unwrap();
    let source = payload.join("fake-query-host.rs");
    fs::write(
        &source,
        r#"
use std::{env, fs, fs::OpenOptions, io::Write, process::{self, Command, Stdio}, thread, time::Duration};

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.first().map(String::as_str) == Some("--descendant") {
        let marker = env::var("KAT_FAKE_DESCENDANT_MARKER").unwrap();
        loop {
            OpenOptions::new().create(true).append(true).open(&marker).unwrap()
                .write_all(b"alive\n").unwrap();
            thread::sleep(Duration::from_millis(100));
        }
    }
    if arguments.len() != 11 || arguments[7] != "--request" || arguments[9] != "--response" {
        process::exit(91);
    }
    let request = fs::read_to_string(&arguments[8]).unwrap();
    if !request.contains("\"operation\":\"query_run\"") {
        process::exit(92);
    }
    fs::write(env::var("KAT_CAPTURE_REQUEST").unwrap(), request).unwrap();
    if env::var_os("KAT_FAKE_QUERY_HANG").is_some() {
        Command::new(env::current_exe().unwrap())
            .arg("--descendant")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        println!("before query timeout");
        std::io::stdout().flush().unwrap();
        thread::sleep(Duration::from_secs(30));
    }
    let response = if env::var_os("KAT_FAKE_LARGE_QUERY").is_some() {
        format!(
            "{{\"status\":\"success\",\"result\":{{\"columns\":[{{\"name\":\"value\",\"type\":\"string\"}}],\"rows\":[[\"{}\"]]}}}}",
            "x".repeat(300_000)
        )
    } else {
        env::var("KAT_FAKE_RUNTIME_RESPONSE").unwrap()
    };
    fs::write(&arguments[10], response).unwrap();
}
"#,
    )
    .unwrap();
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = Command::new(rustc)
        .arg(&source)
        .arg("-o")
        .arg(&host)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn configure(command: &mut Command, root: &Path) {
    command
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("HOME", root.join("home"))
        .env("APPDATA", root.join("app-data"))
        .env("LOCALAPPDATA", root.join("local-app-data"))
        .env("USERPROFILE", root.join("profile"));
}

fn data_home(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("app-data/KAT/data")
    } else {
        root.join("xdg-data/kat")
    }
}

fn write_manifest(root: &Path, dataset: Option<&Path>) -> PathBuf {
    let run = data_home(root).join("runs").join(RUN_ID);
    fs::create_dir_all(&run).unwrap();
    let mut manifest = serde_json::json!({
        "run_id": RUN_ID,
        "pack": "alpha",
        "workflow": "analyze",
        "inputs": {},
        "outputs": {
            "main": {
                "output_id": OUTPUT_ID,
                "columns": [{"name":"value","type":"int64"}],
                "row_count": 1
            }
        }
    });
    if let Some(dataset) = dataset {
        manifest["dataset"] = serde_json::Value::String(dataset.to_str().unwrap().to_owned());
    }
    fs::write(
        run.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    run
}

fn command(binary: &Path, root: &Path, captured: &Path, sql: &str) -> Command {
    let mut command = Command::new(binary);
    command
        .arg("query")
        .args(["--run", RUN_ID, "--sql", sql])
        .env("KAT_CAPTURE_REQUEST", captured)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"columns":[{"name":"value","type":"int64"},{"name":"amount","type":"decimal128(10, 3)"}],"rows":[["9223372036854775807",{"decimal":{"bits":128,"unscaled":"123450","precision":10,"scale":3}}]]}}"#,
        );
    configure(&mut command, root);
    command
}

#[test]
fn query_reads_only_the_final_manifest_and_keeps_sql_and_private_result_boundaries() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = stage_skill(temporary.path());
    let run = write_manifest(temporary.path(), None);
    let captured = temporary.path().join("request.json");
    let sql = "SELECT\n  value\nFROM output.main";
    let before = fs::read_dir(&run).unwrap().count();

    let output = command(&binary, temporary.path(), &captured, sql)
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
    assert_eq!(
        response["result"],
        serde_json::json!({
            "dataset":{"status":"not_provided"},
            "columns":[
                {"name":"value","type":"int64"},
                {"name":"amount","type":"decimal128(10, 3)"}
            ],
            "rows":[["9223372036854775807", "123.450"]]
        })
    );
    let request: serde_json::Value = serde_json::from_slice(&fs::read(captured).unwrap()).unwrap();
    assert_eq!(request["sql"], sql);
    assert_eq!(
        request["dataset"],
        serde_json::json!({"status":"not_provided"})
    );
    assert_eq!(request["outputs"], serde_json::json!({"main":OUTPUT_ID}));
    assert_eq!(request.as_object().unwrap().len(), 6);
    assert_eq!(fs::read_dir(run).unwrap().count(), before);
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert!(log.contains("operation: kat query"));
    assert!(log.contains(r#"sql: "SELECT\n  value\nFROM output.main""#));
}

#[test]
fn query_projects_available_and_unavailable_dataset_states_without_runtime_echo() {
    for available in [true, false] {
        let temporary = tempfile::tempdir().unwrap();
        let binary = stage_skill(temporary.path());
        let dataset = temporary
            .path()
            .join(if available { "dataset" } else { "removed" });
        if available {
            fs::create_dir_all(dataset.join("tables")).unwrap();
            fs::write(dataset.join(".kat-dataset"), []).unwrap();
            fs::write(
                dataset.join("tables/data_dict.parquet"),
                base64::engine::general_purpose::STANDARD
                    .decode(PARQUET)
                    .unwrap(),
            )
            .unwrap();
        }
        let dataset = if available {
            dunce::canonicalize(dataset).unwrap()
        } else {
            dataset
        };
        write_manifest(temporary.path(), Some(&dataset));
        let captured = temporary.path().join("request.json");

        let output = command(
            &binary,
            temporary.path(),
            &captured,
            "SELECT * FROM output.main",
        )
        .output()
        .unwrap();

        assert_eq!(output.status.code(), Some(0));
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let request: serde_json::Value =
            serde_json::from_slice(&fs::read(captured).unwrap()).unwrap();
        if available {
            assert_eq!(response["result"]["dataset"]["status"], "available");
            assert_eq!(
                response["result"]["dataset"]["path"],
                dataset.to_str().unwrap()
            );
            assert!(response["result"]["dataset"].get("tables").is_none());
            assert_eq!(request["dataset"]["status"], "available");
            assert!(
                request["dataset"]["tables"]["data_dict"]
                    .as_str()
                    .unwrap()
                    .ends_with("data_dict.parquet")
            );
        } else {
            assert_eq!(response["result"]["dataset"]["status"], "unavailable");
            assert_eq!(
                response["result"]["dataset"]["path"],
                dataset.to_str().unwrap()
            );
            assert!(
                response["result"]["dataset"]["cause"]
                    .as_str()
                    .unwrap()
                    .contains("Dataset")
            );
            assert_eq!(request["dataset"], response["result"]["dataset"]);
        }
    }
}

#[test]
fn missing_candidate_and_corrupt_manifest_never_start_the_runtime() {
    for corrupt in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let binary = stage_skill(temporary.path());
        if corrupt {
            let run = write_manifest(temporary.path(), None);
            fs::write(run.join("manifest.json"), r#"{"unknown":true}"#).unwrap();
        } else {
            fs::create_dir_all(data_home(temporary.path()).join("runs").join(RUN_ID)).unwrap();
        }
        let captured = temporary.path().join("unexpected-request.json");

        let output = command(
            &binary,
            temporary.path(),
            &captured,
            "SELECT * FROM output.main",
        )
        .output()
        .unwrap();

        assert_eq!(output.status.code(), Some(1));
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(response.get("result").is_none());
        assert_eq!(
            response["error"]["message"],
            if corrupt {
                "Run is corrupted"
            } else {
                "Run 019f6e00-0000-7000-8000-000000000031 does not exist"
            }
        );
        assert!(!captured.exists());
        assert!(response.get("log_path").is_some());
    }
}

#[test]
fn strict_private_dto_and_final_response_byte_limit_fail_whole() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = stage_skill(temporary.path());
    write_manifest(temporary.path(), None);
    let captured = temporary.path().join("request.json");
    let mut invalid = command(
        &binary,
        temporary.path(),
        &captured,
        "SELECT * FROM output.main",
    );
    invalid.env(
        "KAT_FAKE_RUNTIME_RESPONSE",
        r#"{"status":"success","result":{"dataset":{"status":"not_provided"},"columns":[],"rows":[]}}"#,
    );
    let invalid = invalid.output().unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    let invalid_response: serde_json::Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert!(invalid_response.get("result").is_none());

    let mut invalid_decimal = command(
        &binary,
        temporary.path(),
        &captured,
        "SELECT * FROM output.main",
    );
    invalid_decimal.env(
        "KAT_FAKE_RUNTIME_RESPONSE",
        r#"{"status":"success","result":{"columns":[{"name":"amount","type":"decimal128(2, 0)"}],"rows":[[{"decimal":{"bits":128,"unscaled":"1000","precision":2,"scale":0}}]]}}"#,
    );
    let invalid_decimal = invalid_decimal.output().unwrap();
    assert_eq!(invalid_decimal.status.code(), Some(1));
    let invalid_decimal_response: serde_json::Value =
        serde_json::from_slice(&invalid_decimal.stdout).unwrap();
    assert!(invalid_decimal_response.get("result").is_none());

    let mut large = command(
        &binary,
        temporary.path(),
        &captured,
        "SELECT value FROM output.main",
    );
    large.env("KAT_FAKE_LARGE_QUERY", "1");
    let large = large.output().unwrap();
    assert_eq!(large.status.code(), Some(1));
    let large_response: serde_json::Value = serde_json::from_slice(&large.stdout).unwrap();
    assert!(large_response.get("result").is_none());
    assert!(
        large_response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("byte limit exceeded")
    );
    assert!(large.stdout.len() < 4_096);
}

#[test]
fn query_hard_timeout_reaps_a_stuck_runtime_without_partial_result() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = stage_skill(temporary.path());
    write_manifest(temporary.path(), None);
    let captured = temporary.path().join("request.json");
    let marker = temporary.path().join("descendant-heartbeat");
    let mut hanging = command(
        &binary,
        temporary.path(),
        &captured,
        "SELECT * FROM output.main",
    );
    hanging
        .env("KAT_FAKE_QUERY_HANG", "1")
        .env("KAT_FAKE_DESCENDANT_MARKER", &marker);
    let started = Instant::now();

    let output = hanging.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(started.elapsed() < Duration::from_secs(20));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(response.get("result").is_none());
    assert!(response["error"].to_string().contains("hard time limit"));
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert!(log.contains("before query timeout"));
    let heartbeat_size = fs::metadata(&marker)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    thread::sleep(Duration::from_millis(500));
    assert_eq!(
        fs::metadata(&marker)
            .map(|metadata| metadata.len())
            .unwrap_or(0),
        heartbeat_size,
        "Runtime descendant survived the hard timeout"
    );
}

#[test]
fn invalid_run_id_cannot_inject_operation_log_lines_or_terminal_controls() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = stage_skill(temporary.path());
    let captured = temporary.path().join("unexpected-request.json");
    let malicious = "bad\nforged: success\u{1b}[31m";
    let mut invocation = Command::new(binary);
    invocation
        .arg("query")
        .args(["--run", malicious, "--sql", "SELECT 1"])
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env("KAT_FAKE_RUNTIME_RESPONSE", "unused");
    configure(&mut invocation, temporary.path());

    let output = invocation.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    let terminal = String::from_utf8(output.stderr).unwrap();
    assert!(!log.contains('\u{1b}'));
    assert!(!log.lines().any(|line| line == "forged: success"));
    assert!(log.contains(r#"run: "bad\nforged: success\u{1b}[31m""#));
    assert!(!terminal.contains('\u{1b}'));
    assert!(!terminal.contains('\0'));
    assert!(!terminal.lines().any(|line| line == "forged: success"));
    assert!(terminal.contains(r"bad\nforged: success\u{1b}[31m"));
    assert!(!captured.exists());
}
