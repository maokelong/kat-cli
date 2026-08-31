use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use base64::Engine;

#[path = "support/test_home.rs"]
mod test_home;

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

fn json_string_field(document: &str, field: &str) -> String {
    let prefix = format!("\"{field}\":\"");
    let remainder = document.split_once(&prefix).unwrap().1;
    let mut value = String::new();
    let mut characters = remainder.chars();
    while let Some(character) = characters.next() {
        match character {
            '"' => return value,
            '\\' => match characters.next().unwrap() {
                '"' => value.push('"'),
                '\\' => value.push('\\'),
                '/' => value.push('/'),
                'b' => value.push('\u{0008}'),
                'f' => value.push('\u{000c}'),
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                _ => process::exit(93),
            },
            other => value.push(other),
        }
    }
    process::exit(94)
}

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 11 || arguments[7] != "--request" || arguments[9] != "--response" {
        process::exit(91);
    }
    let request = fs::read_to_string(&arguments[8]).unwrap();
    if !request.contains("\"operation\":\"query_run\"") {
        process::exit(92);
    }
    fs::write(env::var("KAT_CAPTURE_REQUEST").unwrap(), &request).unwrap();
    let result_path = json_string_field(&request, "result_path");
    match env::var("KAT_FAKE_RESULT_MODE").as_deref() {
        Ok("none") => {}
        Ok("directory") => fs::create_dir(&result_path).unwrap(),
        Ok(_) => process::exit(95),
        Err(_) => fs::write(
            &result_path,
            env::var("KAT_FAKE_RESULT").unwrap_or_else(|_| "{\"value\":1}\n".into()),
        )
        .unwrap(),
    }
    if env::var_os("KAT_FAKE_EXIT_AFTER_RESULT").is_some() {
        process::exit(96);
    }
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
    test_home::data_home(root)
}

fn write_manifest(root: &Path, dataset: Option<serde_json::Value>) -> PathBuf {
    let run = data_home(root).join("runs").join(RUN_ID);
    fs::create_dir_all(run.join("outputs")).unwrap();
    fs::write(
        run.join("outputs/main.parquet"),
        base64::engine::general_purpose::STANDARD
            .decode(PARQUET)
            .unwrap(),
    )
    .unwrap();
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
        manifest["dataset"] = dataset;
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
    test_home::configure(&mut command, root);
    command
        .arg("query")
        .args(["--run", RUN_ID, "--sql", sql])
        .env("KAT_CAPTURE_REQUEST", captured)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"columns":[{"name":"value","type":"int64"},{"name":"amount","type":"decimal128(10, 3)"}]}}"#,
        );
    command
}

#[test]
fn query_reads_final_manifest_and_sends_only_runtime_inputs() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = stage_skill(temporary.path());
    let run = write_manifest(
        temporary.path(),
        Some(serde_json::json!({"legacy": ["invalid", 42]})),
    );
    let manifest_path = run.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["outputs"]["summary"] = serde_json::json!({
        "columns": [{"name":"count","type":"int64"}],
        "row_count": 1
    });
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::copy(
        run.join("outputs/main.parquet"),
        run.join("outputs/summary.parquet"),
    )
    .unwrap();
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
            "format":"ndjson",
            "path": response["result"]["path"],
            "columns":[
                {"name":"value","type":"int64"},
                {"name":"amount","type":"decimal128(10, 3)"}
            ]
        })
    );
    let result_path = Path::new(response["result"]["path"].as_str().unwrap());
    assert_eq!(fs::read(result_path).unwrap(), b"{\"value\":1}\n");
    assert_eq!(
        result_path.parent().unwrap().file_name().unwrap(),
        "query-results"
    );
    let result_name = result_path.file_name().unwrap().to_str().unwrap();
    let operation_id = result_name
        .strip_prefix("query-")
        .and_then(|name| name.strip_suffix(".ndjson"))
        .unwrap();
    let identity = uuid::Uuid::parse_str(operation_id).unwrap();
    assert_eq!(identity.get_version_num(), 7);
    let request: serde_json::Value = serde_json::from_slice(&fs::read(captured).unwrap()).unwrap();
    assert_eq!(request["sql"], sql);
    assert_eq!(
        request["outputs"],
        serde_json::json!({
            "main": dunce::canonicalize(run.join("outputs/main.parquet")).unwrap(),
            "summary": dunce::canonicalize(run.join("outputs/summary.parquet")).unwrap(),
        })
    );
    assert_eq!(request.as_object().unwrap().len(), 4);
    assert!(request.get("dataset").is_none());
    assert!(request.get("run_path").is_none());
    assert_eq!(request["result_path"], response["result"]["path"]);
    assert_eq!(fs::read_dir(run).unwrap().count(), before);
    let log_path = Path::new(response["log_path"].as_str().unwrap());
    assert_eq!(
        log_path.file_name().unwrap().to_str().unwrap(),
        format!("query-{operation_id}.log")
    );
    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains("operation: kat query"));
    assert!(log.contains(r#"sql: "SELECT\n  value\nFROM output.main""#));
    assert!(log.contains(r#"outputs: ["main","summary"]"#));
    assert!(!log.contains("dataset"));
    assert!(log.contains("runtime_status: success"));
    assert_eq!(
        log.lines()
            .filter(|line| line.starts_with("status: "))
            .collect::<Vec<_>>(),
        ["status: success"]
    );

    let second_capture = temporary.path().join("second-request.json");
    let second = command(&binary, temporary.path(), &second_capture, sql)
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(0));
    let second_response: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_ne!(
        second_response["result"]["path"],
        response["result"]["path"]
    );
    assert_ne!(second_response["log_path"], response["log_path"]);
    assert!(Path::new(second_response["result"]["path"].as_str().unwrap()).is_file());
}

#[test]
fn query_ignores_every_historical_manifest_dataset_shape() {
    for dataset in [
        serde_json::Value::Null,
        serde_json::json!(false),
        serde_json::json!(17),
        serde_json::json!("relative/or/missing"),
        serde_json::json!(["unexpected"]),
        serde_json::json!({"unexpected": true}),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let binary = stage_skill(temporary.path());
        write_manifest(temporary.path(), Some(dataset));
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
        assert_eq!(response["result"]["format"], "ndjson");
        assert!(response["result"].get("dataset").is_none());
        assert!(request.get("dataset").is_none());
        assert!(!log.contains("dataset"));
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

#[test]
fn controlled_failures_remove_the_candidate_and_retain_the_operation_log() {
    for (name, result_mode, runtime_response, exits_after_result) in [
        (
            "runtime failure",
            None,
            r#"{"status":"failure","error":{"message":"query failed"}}"#,
            false,
        ),
        ("invalid response", None, "not-json", false),
        (
            "removed rows field",
            None,
            r#"{"status":"success","result":{"columns":[],"rows":[]}}"#,
            false,
        ),
        (
            "missing file",
            Some("none"),
            r#"{"status":"success","result":{"columns":[]}}"#,
            false,
        ),
        (
            "directory instead of file",
            Some("directory"),
            r#"{"status":"success","result":{"columns":[]}}"#,
            false,
        ),
        (
            "host exit",
            None,
            r#"{"status":"success","result":{"columns":[]}}"#,
            true,
        ),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let binary = stage_skill(temporary.path());
        write_manifest(temporary.path(), None);
        let captured = temporary.path().join("request.json");
        let mut query = command(
            &binary,
            temporary.path(),
            &captured,
            "SELECT * FROM output.main",
        );
        query.env("KAT_FAKE_RUNTIME_RESPONSE", runtime_response);
        if let Some(result_mode) = result_mode {
            query.env("KAT_FAKE_RESULT_MODE", result_mode);
        }
        if exits_after_result {
            query.env("KAT_FAKE_EXIT_AFTER_RESULT", "1");
        }

        let output = query.output().unwrap();

        assert_eq!(
            output.status.code(),
            Some(1),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let request: serde_json::Value =
            serde_json::from_slice(&fs::read(&captured).unwrap()).unwrap();
        assert!(
            !Path::new(request["result_path"].as_str().unwrap()).exists(),
            "{name} left its candidate"
        );
        assert!(
            Path::new(response["log_path"].as_str().unwrap()).is_file(),
            "{name} lost its Operation log"
        );
    }
}
