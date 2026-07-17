use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use base64::Engine;

const PARQUET: &str = "UEFSMRUEFSAVIEwVBBUAEgAAAQAAAAAAAAACAAAAAAAAABUAFRIVEiwVBBUQFQYVBgAAAgAAAAQBAQMCFQQVMBUwTBUEFQASAAAGAAAAY2FsbGVyCgAAAGZ1dGV4X3dhaXQVABUSFRIsFQQVEBUGFQYAAAIAAAAEAQEDAhkSAhkYCAEAAAAAAAAAGRgIAgAAAAAAAAAVAhkWACkmAAQAGRICGRgGY2FsbGVyGRgKZnV0ZXhfd2FpdBUCGRYAKSYABAAZHBZEFTQWAAAAGRwWxAEVNBYAABkWIAAVAhk8SAxhcnJvd19zY2hlbWEVBAAVBCUCGAJpZAAVDCUCGARkYXRhJQBMHAAAABYEGRwZLCYAHBUEGTUABhAZGAJpZBUAFgQWcBZwJkQmCBwYCAIAAAAAAAAAGAgBAAAAAAAAABYAKAgCAAAAAAAAABgIAQAAAAAAAAAREQAZLBUEFQAVAgAVABUQFQIAPDkmAAQAABaEAxUUFvgBFUYAJgAcFQwZNQAGEBkYBGRhdGEVABYEFoABFoABJsQBJngcNgAoCmZ1dGV4X3dhaXQYBmNhbGxlchERABksFQQVABUCABUAFRAVAgA8FiApJgAEAAAWmAMVHBa+AhVGABbwARYEJggW8AEUAAAZHBgMQVJST1c6c2NoZW1hGOwBLy8vLy82Z0FBQUFRQUFBQUFBQUtBQXdBQ2dBSkFBUUFDZ0FBQUJBQUFBQUFBUVFBQ0FBSUFBQUFCQUFJQUFBQUJBQUFBQUlBQUFCRUFBQUFCQUFBQU5ULy8vOFlBQUFBREFBQUFBQUFBUVVRQUFBQUFBQUFBQVFBQkFBRUFBQUFCQUFBQUdSaGRHRUFBQUFBRUFBVUFCQUFEZ0FQQUFRQUFBQUlBQkFBQUFBWUFBQUFJQUFBQUFBQUFRSWNBQUFBQ0FBTUFBUUFDd0FJQUFBQVFBQUFBQUFBQUFFQUFBQUFBZ0FBQUdsa0FBQT0AGBlwYXJxdWV0LXJzIHZlcnNpb24gNTguMy4wGSwcAAAcAAAALwIAAFBBUjE=";

fn cargo_kat() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kat"))
}

fn stage_skill(root: &Path) -> (PathBuf, PathBuf) {
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
    (skill, binary)
}

fn stage_fake_host(binary: &Path) {
    let payload = binary.parent().unwrap();
    let host = if cfg!(windows) {
        payload.join("python/python.exe")
    } else {
        payload.join("python/bin/python3")
    };
    fs::create_dir_all(host.parent().unwrap()).unwrap();
    let source = payload.join("fake-run-host.rs");
    fs::write(
        &source,
        r#"
use std::{env, fs, path::Path, process};

fn json_string(document: &str, key: &str) -> String {
    let marker = format!("\"{key}\":\"");
    let value = document.split_once(&marker).unwrap().1;
    let mut decoded = String::new();
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            decoded.push(match character {
                '\\' => '\\',
                '"' => '"',
                '/' => '/',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                _ => panic!("unsupported test JSON escape"),
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return decoded;
        } else {
            decoded.push(character);
        }
    }
    panic!("unterminated test JSON string")
}

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 11 || arguments[7] != "--request" || arguments[9] != "--response" {
        process::exit(91);
    }
    let request = fs::read_to_string(&arguments[8]).unwrap();
    if !request.contains("\"operation\":\"run_workflow\"")
        || !request.contains("\"candidate_id\":")
        || !request.contains("\"run_path\":")
    {
        process::exit(92);
    }
    fs::write(env::var("KAT_CAPTURE_REQUEST").unwrap(), &request).unwrap();
    if env::var_os("KAT_FAKE_MANIFEST_DIRECTORY").is_some() {
        let run_path = json_string(&request, "run_path");
        fs::create_dir(Path::new(&run_path).join("manifest.json")).unwrap();
    }
    println!("fake Workflow output");
    fs::write(&arguments[10], env::var("KAT_FAKE_RUNTIME_RESPONSE").unwrap()).unwrap();
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

fn pack(root: &Path) -> PathBuf {
    let pack = root.join("pack");
    fs::create_dir_all(&pack).unwrap();
    fs::write(
        pack.join("pack.toml"),
        "name = \"alpha\"\ntitle = \"Alpha\"\ndescription = \"Run fixture\"\nowner = \"Test\"\n",
    )
    .unwrap();
    pack
}

fn dataset(root: &Path) -> PathBuf {
    let dataset = root.join("dataset");
    fs::create_dir_all(dataset.join("tables")).unwrap();
    fs::write(dataset.join(".kat-dataset"), []).unwrap();
    fs::write(
        dataset.join("tables/data_dict.parquet"),
        base64::engine::general_purpose::STANDARD
            .decode(PARQUET)
            .unwrap(),
    )
    .unwrap();
    dunce::canonicalize(dataset).unwrap()
}

#[test]
fn run_publishes_one_manifest_and_only_public_output_facts() {
    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let pack = pack(temporary.path());
    let dataset = dataset(temporary.path());
    let captured = temporary.path().join("request.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args(["--pack", "alpha", "--workflow", "analyze"])
        .arg("--pack-dir")
        .arg(&pack)
        .arg("--dataset")
        .arg(&dataset)
        .arg("--")
        .args(["--limit", "5"])
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{"limit":"5"},"outputs":{"main":{"output_id":"0123456789abcdef0123456789abcdef","columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "success");
    let run_id = response["result"]["run_id"].as_str().unwrap();
    assert_eq!(
        response["result"]["outputs"],
        serde_json::json!({"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}})
    );
    assert!(response["result"].get("inputs").is_none());
    assert!(response["result"].get("dataset").is_none());
    assert!(response.to_string().find("output_id").is_none());

    let manifest_path = data_home(temporary.path())
        .join("runs")
        .join(run_id)
        .join("manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["run_id"], run_id);
    assert_eq!(manifest["pack"], "alpha");
    assert_eq!(manifest["workflow"], "analyze");
    assert_eq!(manifest["dataset"], dataset.to_str().unwrap());
    assert_eq!(manifest["inputs"], serde_json::json!({"limit":"5"}));
    assert_eq!(
        manifest["outputs"]["main"]["output_id"],
        "0123456789abcdef0123456789abcdef"
    );

    let request: serde_json::Value = serde_json::from_slice(&fs::read(captured).unwrap()).unwrap();
    assert_eq!(request["candidate_id"], run_id);
    assert_eq!(request["workflow_name"], "analyze");
    assert_eq!(request["arguments"], serde_json::json!(["--limit", "5"]));
    assert_eq!(request["dataset"]["path"], dataset.to_str().unwrap());
    assert!(
        request["dataset"]["tables"]["data_dict"]
            .as_str()
            .unwrap()
            .ends_with("data_dict.parquet")
    );
    let log_path = PathBuf::from(response["log_path"].as_str().unwrap());
    assert_eq!(
        log_path.file_name().unwrap().to_str().unwrap(),
        format!("run-{run_id}.log")
    );
    assert!(
        fs::read_to_string(log_path)
            .unwrap()
            .contains("fake Workflow output")
    );
}

#[test]
fn runtime_failure_never_publishes_or_exposes_the_candidate() {
    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let pack = pack(temporary.path());
    let captured = temporary.path().join("failed-request.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args(["--pack", "alpha", "--workflow", "analyze"])
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"failure","error":{"message":"Workflow execution failed","causes":["expected failure"],"help":"Correct the Workflow"}}"#,
        );
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    let request: serde_json::Value = serde_json::from_slice(&fs::read(captured).unwrap()).unwrap();
    let candidate_id = request["candidate_id"].as_str().unwrap();
    assert!(!response["error"].to_string().contains(candidate_id));
    assert!(
        !data_home(temporary.path())
            .join("runs")
            .join(candidate_id)
            .join("manifest.json")
            .exists()
    );
}

#[test]
fn operation_log_creation_failure_never_starts_runtime_or_publishes() {
    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let pack = pack(temporary.path());
    let captured = temporary.path().join("unexpected-request.json");
    fs::create_dir_all(data_home(temporary.path())).unwrap();
    fs::write(data_home(temporary.path()).join("logs"), "not a directory").unwrap();
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args(["--pack", "alpha", "--workflow", "analyze"])
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"output_id":"0123456789abcdef0123456789abcdef","columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    assert!(!captured.exists());
    assert!(!data_home(temporary.path()).join("runs").exists());
    assert!(response.get("log_path").is_none());
}

#[test]
fn candidate_creation_failure_is_completed_through_its_run_log() {
    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let pack = pack(temporary.path());
    let captured = temporary.path().join("unexpected-candidate-request.json");
    fs::create_dir_all(data_home(temporary.path())).unwrap();
    fs::write(data_home(temporary.path()).join("runs"), "not a directory").unwrap();
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args(["--pack", "alpha", "--workflow", "analyze"])
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"output_id":"0123456789abcdef0123456789abcdef","columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    assert!(!captured.exists());
    let log_path = PathBuf::from(response["log_path"].as_str().unwrap());
    let candidate_id = log_path
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .strip_prefix("run-")
        .unwrap()
        .to_owned();
    assert!(!response["error"].to_string().contains(&candidate_id));
    assert!(
        fs::read_to_string(log_path)
            .unwrap()
            .contains("failed to create Run root")
    );
}

#[test]
fn manifest_publication_failure_never_returns_or_publishes_a_run() {
    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let pack = pack(temporary.path());
    let captured = temporary.path().join("manifest-fault-request.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args(["--pack", "alpha", "--workflow", "analyze"])
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env("KAT_FAKE_MANIFEST_DIRECTORY", "1")
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"output_id":"0123456789abcdef0123456789abcdef","columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    let request: serde_json::Value = serde_json::from_slice(&fs::read(captured).unwrap()).unwrap();
    let candidate_id = request["candidate_id"].as_str().unwrap();
    assert!(!response["error"].to_string().contains(candidate_id));
    let manifest_entry = data_home(temporary.path())
        .join("runs")
        .join(candidate_id)
        .join("manifest.json");
    assert!(manifest_entry.is_dir());
    assert!(!manifest_entry.is_file());
}
