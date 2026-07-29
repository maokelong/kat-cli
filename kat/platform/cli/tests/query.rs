use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use base64::Engine;

const RUN_ID: &str = "019f6e00-0000-7000-8000-000000000031";
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
    let data_home = data_home(root);
    fs::create_dir_all(&data_home).unwrap();
    fs::write(
        skill.join("config.json"),
        serde_json::json!({ "kat_data_home": data_home }).to_string(),
    )
    .unwrap();
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
use std::{env, fs, process};

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 11 || arguments[7] != "--request" || arguments[9] != "--response" {
        process::exit(91);
    }
    let request = fs::read_to_string(&arguments[8]).unwrap();
    if !request.contains("\"operation\":\"query_run\"") {
        process::exit(92);
    }
    fs::write(env::var("KAT_CAPTURE_REQUEST").unwrap(), request).unwrap();
    let response = env::var("KAT_FAKE_RUNTIME_RESPONSE").unwrap();
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

fn command(binary: &Path, _root: &Path, captured: &Path, sql: &str) -> Command {
    let mut command = Command::new(binary);
    command
        .arg("query")
        .args(["--run", RUN_ID, "--sql", sql])
        .env("KAT_CAPTURE_REQUEST", captured)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"columns":[{"name":"value","type":"int64"},{"name":"amount","type":"decimal128(10, 3)"}],"rows":[["9223372036854775807","123.450"]]}}"#,
        );
    command
}

#[test]
fn query_reads_final_manifest_and_sends_only_runtime_inputs() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = stage_skill(temporary.path());
    let run = write_manifest(temporary.path(), None);
    let manifest_path = run.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["outputs"]["summary"] = serde_json::json!({
        "columns": [{"name":"count","type":"int64"}],
        "row_count": 1
    });
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
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
    assert_eq!(request["outputs"], serde_json::json!(["main", "summary"]));
    assert_eq!(request.as_object().unwrap().len(), 4);
    assert!(request.get("dataset").is_none());
    assert!(request.get("run_id").is_none());
    assert_eq!(fs::read_dir(run).unwrap().count(), before);
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert!(log.contains("operation: kat query"));
    assert!(log.contains(r#"sql: "SELECT\n  value\nFROM output.main""#));
    assert!(log.contains(r#"outputs: ["main","summary"]"#));
    assert!(log.contains("runtime_status: success"));
    assert_eq!(
        log.lines()
            .filter(|line| line.starts_with("status: "))
            .collect::<Vec<_>>(),
        ["status: success"]
    );
}

#[test]
fn query_projects_dataset_state_and_only_sends_available_dataset() {
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
        let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
        assert!(log.contains(&format!("dataset_path: {:?}", dataset.to_str().unwrap())));
        if available {
            assert_eq!(response["result"]["dataset"]["status"], "available");
            assert_eq!(
                response["result"]["dataset"]["path"],
                dataset.to_str().unwrap()
            );
            assert!(response["result"]["dataset"].get("tables").is_none());
            assert_eq!(request["dataset"]["path"], dataset.to_str().unwrap());
            assert!(request["dataset"].get("status").is_none());
            assert!(
                request["dataset"]["tables"]["data_dict"]
                    .as_str()
                    .unwrap()
                    .ends_with("data_dict.parquet")
            );
            assert!(log.contains("dataset_status: available"));
            assert!(!log.contains("dataset_cause:"));
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
            assert!(request.get("dataset").is_none());
            assert!(log.contains("dataset_status: unavailable"));
            assert!(log.contains("dataset_cause:"));
        }
    }
}

#[test]
fn corrupt_manifest_never_starts_runtime() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = stage_skill(temporary.path());
    let run = write_manifest(temporary.path(), None);
    fs::write(run.join("manifest.json"), r#"{"unknown":true}"#).unwrap();
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
    assert_eq!(response["error"]["message"], "Run is corrupted");
    assert!(response.get("result").is_none());
    assert!(!captured.exists());
}
