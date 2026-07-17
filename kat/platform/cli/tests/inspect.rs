use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use base64::Engine;

const DATA_DICT_PARQUET: &str = "UEFSMRUEFSAVIEwVBBUAEgAAAQAAAAAAAAACAAAAAAAAABUAFRIVEiwVBBUQFQYVBgAAAgAAAAQBAQMCFQQVMBUwTBUEFQASAAAGAAAAY2FsbGVyCgAAAGZ1dGV4X3dhaXQVABUSFRIsFQQVEBUGFQYAAAIAAAAEAQEDAhkSAhkYCAEAAAAAAAAAGRgIAgAAAAAAAAAVAhkWACkmAAQAGRICGRgGY2FsbGVyGRgKZnV0ZXhfd2FpdBUCGRYAKSYABAAZHBZEFTQWAAAAGRwWxAEVNBYAABkWIAAVAhk8SAxhcnJvd19zY2hlbWEVBAAVBCUCGAJpZAAVDCUCGARkYXRhJQBMHAAAABYEGRwZLCYAHBUEGTUABhAZGAJpZBUAFgQWcBZwJkQmCBwYCAIAAAAAAAAAGAgBAAAAAAAAABYAKAgCAAAAAAAAABgIAQAAAAAAAAAREQAZLBUEFQAVAgAVABUQFQIAPDkmAAQAABaEAxUUFvgBFUYAJgAcFQwZNQAGEBkYBGRhdGEVABYEFoABFoABJsQBJngcNgAoCmZ1dGV4X3dhaXQYBmNhbGxlchERABksFQQVABUCABUAFRAVAgA8FiApJgAEAAAWmAMVHBa+AhVGABbwARYEJggW8AEUAAAZHBgMQVJST1c6c2NoZW1hGOwBLy8vLy82Z0FBQUFRQUFBQUFBQUtBQXdBQ2dBSkFBUUFDZ0FBQUJBQUFBQUFBUVFBQ0FBSUFBQUFCQUFJQUFBQUJBQUFBQUlBQUFCRUFBQUFCQUFBQU5ULy8vOFlBQUFBREFBQUFBQUFBUVVRQUFBQUFBQUFBQVFBQkFBRUFBQUFCQUFBQUdSaGRHRUFBQUFBRUFBVUFCQUFEZ0FQQUFRQUFBQUlBQkFBQUFBWUFBQUFJQUFBQUFBQUFRSWNBQUFBQ0FBTUFBUUFDd0FJQUFBQVFBQUFBQUFBQUFFQUFBQUFBZ0FBQUdsa0FBQT0AGBlwYXJxdWV0LXJzIHZlcnNpb24gNTguMy4wGSwcAAAcAAAALwIAAFBBUjE=";

fn cargo_kat() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kat"))
}

fn stage_skill(root: &Path) -> (PathBuf, PathBuf) {
    let skill = root.join("movable-skill");
    let target = if cfg!(windows) {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    };
    let binary_name = if cfg!(windows) { "kat.exe" } else { "kat" };
    let payload = skill.join("scripts").join("targets").join(target);
    fs::create_dir_all(&payload).expect("create Platform Payload");
    fs::write(skill.join("SKILL.md"), "# KAT\n").expect("write Skill marker");
    let binary = payload.join(binary_name);
    fs::copy(cargo_kat(), &binary).expect("copy kat into Skill");
    (skill, binary)
}

fn stage_fake_python_host(binary: &Path) {
    let payload = binary.parent().expect("Platform Payload directory");
    let host = if cfg!(windows) {
        payload.join("python").join("python.exe")
    } else {
        payload.join("python").join("bin").join("python3")
    };
    fs::create_dir_all(host.parent().unwrap()).expect("create fake Host directory");
    let source = payload.join("fake-python-host.rs");
    fs::write(
        &source,
        r#"
use std::{env, fs, io::{self, Write}, process};

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let fixed = ["-I", "-B", "-X", "utf8", "-u", "-m", "_kat_runtime", "--request"];
    if arguments.len() != 11
        || arguments[..8] != fixed
        || arguments[9] != "--response"
    {
        process::exit(91);
    }
    let request = fs::read_to_string(&arguments[8]).unwrap();
    if !request.contains("\"operation\":\"inspect_pack\"")
        || !request.contains("\"pack_name\":")
        || !request.contains("\"pack_path\":")
    {
        process::exit(92);
    }
    io::stdout().write_all(b"\x1b[31mruntime stdout\x1b[0m\r\ninvalid: \xff\r").unwrap();
    io::stderr().write_all(b"runtime stderr\r\n").unwrap();
    fs::write(&arguments[10], env::var("KAT_FAKE_RUNTIME_RESPONSE").unwrap()).unwrap();
    process::exit(env::var("KAT_FAKE_RUNTIME_EXIT").unwrap_or_else(|_| "0".to_owned()).parse().unwrap());
}
"#,
    )
    .expect("write fake Host source");
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = Command::new(rustc)
        .arg(&source)
        .arg("-o")
        .arg(&host)
        .output()
        .expect("compile fake Host");
    assert!(
        output.status.success(),
        "fake Host compilation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn configure_platform_directories(command: &mut Command, root: &Path) {
    command
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("HOME", root.join("home"))
        .env("APPDATA", root.join("app-data"))
        .env("LOCALAPPDATA", root.join("local-app-data"))
        .env("USERPROFILE", root.join("profile"));
}

fn data_home(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("app-data").join("KAT").join("data")
    } else {
        root.join("xdg-data").join("kat")
    }
}

fn write_pack(directory: &Path, name: &str, description: &str) -> String {
    fs::create_dir_all(directory).expect("create PACK directory");
    let manifest = format!(
        "name = {name:?}\ntitle = {name:?}\ndescription = {description:?}\nowner = \"Test Team\"\n"
    );
    fs::write(directory.join("pack.toml"), &manifest).expect("write PACK manifest");
    manifest
}

fn targeted_inspect_command(binary: &Path, root: &Path, pack: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .arg("inspect")
        .args(["--pack", "alpha"])
        .arg("--pack-dir")
        .arg(pack)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"workflows":[]}}"#,
        );
    configure_platform_directories(&mut command, root);
    command
}

#[test]
fn targeted_pack_inspection_uses_adjacent_host_and_delivers_clean_log() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_python_host(&binary);
    let pack = temporary.path().join("external-checkout");
    let manifest = write_pack(&pack, "alpha", "External PACK");

    let output = targeted_inspect_command(&binary, temporary.path(), &pack)
        .output()
        .expect("inspect target PACK");

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "success");
    assert_eq!(response["result"]["name"], "alpha");
    assert_eq!(response["result"]["title"], "alpha");
    assert_eq!(response["result"]["description"], "External PACK");
    assert_eq!(response["result"]["owner"], "Test Team");
    assert_eq!(response["result"]["workflows"], serde_json::json!([]));
    assert!(response["result"].get("pack").is_none());
    let log_path = PathBuf::from(response["log_path"].as_str().unwrap());
    let log = fs::read_to_string(log_path).expect("read Operation log");
    assert!(!log.contains('\u{1b}'));
    assert!(log.contains("runtime stdout\n"));
    assert!(log.contains("runtime stderr\n"));
    assert!(log.contains("invalid UTF-8 in Runtime stdout was replaced"));
    assert!(log.contains('�'));
    assert!(log.contains("status: success\n"));
    assert_eq!(
        fs::read_to_string(pack.join("pack.toml")).unwrap(),
        manifest
    );
    assert_eq!(fs::read_dir(&pack).unwrap().count(), 1);
}

#[test]
fn targeted_pack_inspection_enforces_runtime_exit_and_response_matrix() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_python_host(&binary);
    let pack = temporary.path().join("external-checkout");
    write_pack(&pack, "alpha", "External PACK");

    let mut invalid = targeted_inspect_command(&binary, temporary.path(), &pack);
    invalid.env(
        "KAT_FAKE_RUNTIME_RESPONSE",
        r#"{"status":"success","result":{"workflows":[],"extra":true}}"#,
    );
    let invalid = invalid.output().expect("run invalid Runtime Response");
    assert_eq!(invalid.status.code(), Some(1));
    let invalid_response: serde_json::Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(
        invalid_response["error"]["message"],
        "PACK inspection Runtime failed"
    );
    assert!(invalid_response.get("result").is_none());
    assert!(invalid_response.get("log_path").is_some());

    let mut semantic = targeted_inspect_command(&binary, temporary.path(), &pack);
    semantic.env(
        "KAT_FAKE_RUNTIME_RESPONSE",
        r#"{"status":"success","result":{"workflows":[{"name":"bad","title":"Bad","description":"Bad","required_tables":[],"parameters":[{"name":"flag","option":"--flag","negative_option":"--no-flag","type":"boolean","required":false,"description":"Flag","default":null}]}]}}"#,
    );
    let semantic = semantic.output().expect("run invalid Runtime semantics");
    assert_eq!(semantic.status.code(), Some(1));
    let semantic_response: serde_json::Value = serde_json::from_slice(&semantic.stdout).unwrap();
    assert_eq!(
        semantic_response["error"]["message"],
        "PACK inspection Runtime failed"
    );
    assert!(semantic_response.get("result").is_none());

    let mut nonzero = targeted_inspect_command(&binary, temporary.path(), &pack);
    nonzero.env("KAT_FAKE_RUNTIME_EXIT", "7");
    let nonzero = nonzero.output().expect("run nonzero Runtime");
    assert_eq!(nonzero.status.code(), Some(1));
    let nonzero_response: serde_json::Value = serde_json::from_slice(&nonzero.stdout).unwrap();
    assert_eq!(
        nonzero_response["error"]["message"],
        "PACK inspection Runtime failed"
    );
    assert!(nonzero_response.get("result").is_none());

    let mut failure = targeted_inspect_command(&binary, temporary.path(), &pack);
    failure.env(
        "KAT_FAKE_RUNTIME_RESPONSE",
        r#"{"status":"failure","error":{"message":"PACK declaration is invalid","causes":["missing docstring"],"help":"Add a docstring"}}"#,
    );
    let failure = failure.output().expect("run legal Runtime failure");
    assert_eq!(failure.status.code(), Some(1));
    let failure_response: serde_json::Value = serde_json::from_slice(&failure.stdout).unwrap();
    assert_eq!(
        failure_response["error"]["message"],
        "PACK declaration is invalid"
    );
    assert_eq!(
        failure_response["error"]["causes"],
        serde_json::json!(["missing docstring"])
    );
    assert_eq!(failure_response["error"]["help"], "Add a docstring");
    assert!(String::from_utf8_lossy(&failure.stderr).contains("PACK declaration is invalid"));
}

#[test]
fn targeted_pack_inspection_log_creation_failure_does_not_start_runtime() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_python_host(&binary);
    let pack = temporary.path().join("external-checkout");
    write_pack(&pack, "alpha", "External PACK");
    fs::create_dir_all(data_home(temporary.path())).unwrap();
    fs::write(data_home(temporary.path()).join("logs"), "not a directory").unwrap();

    let output = targeted_inspect_command(&binary, temporary.path(), &pack)
        .output()
        .expect("fail log creation");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        "PACK inspection Operation log could not be delivered"
    );
    assert!(response.get("log_path").is_none());
    assert!(response.get("result").is_none());
}

#[test]
fn targeted_pack_preflight_failures_still_deliver_the_single_operation_log() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_skill(temporary.path());
    let pack = temporary.path().join("external-checkout");
    write_pack(&pack, "alpha", "External PACK");
    let mut command = Command::new(&binary);
    command
        .arg("inspect")
        .args(["--pack", "missing"])
        .arg("--pack-dir")
        .arg(&pack);
    configure_platform_directories(&mut command, temporary.path());

    let output = command.output().expect("reject unknown PACK");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        "PACK \"missing\" was not discovered"
    );
    assert!(response.get("result").is_none());
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert!(log.contains("operation: kat inspect --pack"));
    assert!(log.contains("pack: missing"));
    assert!(log.contains("status: failure"));
    assert_eq!(
        fs::read_dir(data_home(temporary.path()).join("logs"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn help_and_parse_failures_do_not_require_a_skill_layout() {
    let help = Command::new(cargo_kat())
        .arg("--help")
        .output()
        .expect("run help");
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let help_text = String::from_utf8(help.stdout).expect("UTF-8 help");
    assert!(help_text.contains("inspect"));
    assert!(!help_text.starts_with('{'));

    let operation_help = Command::new(cargo_kat())
        .args(["inspect", "--help"])
        .output()
        .expect("run operation help");
    assert_eq!(operation_help.status.code(), Some(0));
    assert!(operation_help.stderr.is_empty());
    assert!(!operation_help.stdout.starts_with(b"{"));

    for arguments in [
        Vec::<&str>::new(),
        vec!["unknown"],
        vec!["inspect", "--version"],
    ] {
        let output = Command::new(cargo_kat())
            .args(arguments)
            .output()
            .expect("run parse failure");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn inspect_lists_all_packs_from_a_moved_skill_and_arbitrary_cwd() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (skill, binary) = stage_skill(temporary.path());
    let bundled = skill.join("assets").join("packs").join("bundled-directory");
    let bundled_manifest = write_pack(&bundled, "bravo", "Bundled description");
    let cwd = temporary.path().join("unrelated-cwd");
    let additional = cwd.join("relative-pack");
    let additional_manifest = write_pack(&additional, "alpha", "External description");
    let data_pack = data_home(temporary.path())
        .join("packs")
        .join("data-directory");
    let data_manifest = write_pack(&data_pack, "charlie", "Data Home description");
    let moved_skill = temporary.path().join("moved-again");
    fs::rename(&skill, &moved_skill).expect("move complete Skill");
    let moved_binary = moved_skill.join(binary.strip_prefix(&skill).unwrap());
    let moved_bundled = moved_skill.join(bundled.strip_prefix(&skill).unwrap());

    let mut command = Command::new(moved_binary);
    command
        .current_dir(&cwd)
        .args(["inspect", "--pack-dir", "relative-pack"]);
    configure_platform_directories(&mut command, temporary.path());
    let output = command.output().expect("run staged kat inspect");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"status\":\"success\",\"result\":{\"packs\":[{\"name\":\"alpha\",\"title\":\"alpha\",\"description\":\"External description\",\"owner\":\"Test Team\"},{\"name\":\"bravo\",\"title\":\"bravo\",\"description\":\"Bundled description\",\"owner\":\"Test Team\"},{\"name\":\"charlie\",\"title\":\"charlie\",\"description\":\"Data Home description\",\"owner\":\"Test Team\"}]}}\n"
    );
    assert_eq!(
        fs::read_to_string(moved_bundled.join("pack.toml")).unwrap(),
        bundled_manifest
    );
    assert_eq!(
        fs::read_to_string(additional.join("pack.toml")).unwrap(),
        additional_manifest
    );
    assert_eq!(
        fs::read_to_string(data_pack.join("pack.toml")).unwrap(),
        data_manifest
    );
    assert_eq!(
        fs::read_to_string(moved_skill.join("SKILL.md")).unwrap(),
        "# KAT\n"
    );
    assert_eq!(fs::read_dir(&moved_bundled).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&additional).unwrap().count(), 1);
    assert!(!data_home(temporary.path()).join("logs").exists());
}

#[test]
fn formed_operation_rejects_a_binary_outside_a_complete_skill() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let mut command = Command::new(cargo_kat());
    command.arg("inspect");
    configure_platform_directories(&mut command, temporary.path());

    let output = command.output().expect("run unstaged kat inspect");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert_eq!(response["error"]["message"], "KAT Skill is unavailable");
    assert!(response.get("result").is_none());
    assert!(String::from_utf8_lossy(&output.stderr).contains("KAT Skill is unavailable"));
    assert!(!data_home(temporary.path()).exists());
}

#[test]
fn formed_operation_failure_is_one_json_response_and_readable_diagnostic() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_skill(temporary.path());
    let broken = temporary.path().join("broken-pack");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join("pack.toml"), "not valid TOML = [").unwrap();
    let mut command = Command::new(binary);
    command.arg("inspect").arg("--pack-dir").arg(&broken);
    configure_platform_directories(&mut command, temporary.path());

    let output = command.output().expect("run failed inspection");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).expect("failure JSON");
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    assert!(response.get("log_path").is_none());
    assert_eq!(response["error"]["message"], "PACK discovery failed");
    assert!(response["error"]["causes"].as_array().unwrap().len() >= 2);
    assert!(String::from_utf8_lossy(&output.stderr).contains("PACK discovery failed"));
    assert!(!data_home(temporary.path()).join("logs").exists());
}

#[test]
fn absent_default_directories_are_an_empty_result_and_are_not_created() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (skill, binary) = stage_skill(temporary.path());
    let mut command = Command::new(binary);
    command.arg("inspect");
    configure_platform_directories(&mut command, temporary.path());

    let output = command.output().expect("run empty inspection");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"status\":\"success\",\"result\":{\"packs\":[]}}\n"
    );
    assert!(!skill.join("assets").join("packs").exists());
    assert!(!data_home(temporary.path()).exists());
}

#[test]
fn closed_stdout_makes_the_real_process_fail() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (skill, binary) = stage_skill(temporary.path());
    write_pack(
        &skill.join("assets").join("packs").join("large"),
        "large",
        &"x".repeat(2 * 1024 * 1024),
    );
    let mut command = Command::new(binary);
    command
        .arg("inspect")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_platform_directories(&mut command, temporary.path());
    let mut child = command.spawn().expect("spawn kat inspect");
    drop(child.stdout.take());

    let output = child.wait_with_output().expect("wait for kat inspect");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("write KAT Response"));
}

#[test]
fn dataset_inspection_uses_cwd_and_does_not_touch_pack_or_data_home_state() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (skill, binary) = stage_skill(temporary.path());
    let cwd = temporary.path().join("cwd");
    let dataset = cwd.join("relative-dataset");
    fs::create_dir_all(&dataset).unwrap();
    fs::write(dataset.join(".kat-dataset"), []).unwrap();
    fs::write(dataset.join("notes.txt"), "ignored").unwrap();
    fs::create_dir(dataset.join("tables")).unwrap();
    fs::write(
        dataset.join("tables/data_dict.parquet"),
        base64::engine::general_purpose::STANDARD
            .decode(DATA_DICT_PARQUET)
            .unwrap(),
    )
    .unwrap();
    let mut command = Command::new(binary);
    command
        .current_dir(&cwd)
        .args(["inspect", "--dataset", "relative-dataset"]);
    configure_platform_directories(&mut command, temporary.path());

    let output = command.output().expect("inspect Dataset");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "success");
    assert_eq!(
        response["result"]["path"],
        dunce::canonicalize(&dataset).unwrap().to_str().unwrap()
    );
    assert_eq!(
        response["result"]["tables"],
        serde_json::json!([{
            "name": "data_dict",
            "columns": [
                {"name": "id", "type": "Int64", "nullable": true},
                {"name": "data", "type": "Utf8", "nullable": true}
            ]
        }])
    );
    assert!(response.get("log_path").is_none());
    assert!(!skill.join("assets").join("packs").exists());
    assert!(!data_home(temporary.path()).exists());
    assert_eq!(
        fs::read_to_string(dataset.join("notes.txt")).unwrap(),
        "ignored"
    );
}

#[test]
fn dataset_inspection_failure_and_argument_conflict_keep_process_contract() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_skill(temporary.path());
    let dataset = temporary.path().join("invalid-dataset");
    fs::create_dir(&dataset).unwrap();
    let mut command = Command::new(&binary);
    command.arg("inspect").arg("--dataset").arg(&dataset);
    configure_platform_directories(&mut command, temporary.path());

    let output = command.output().expect("inspect invalid Dataset");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["error"]["message"], "Dataset inspection failed");
    assert!(response.get("result").is_none());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Dataset inspection failed"));

    let corrupt = temporary.path().join("corrupt-dataset");
    fs::create_dir_all(corrupt.join("tables")).unwrap();
    fs::write(corrupt.join(".kat-dataset"), []).unwrap();
    fs::write(corrupt.join("tables/events.parquet"), "broken").unwrap();
    let mut corrupt_command = Command::new(&binary);
    corrupt_command
        .arg("inspect")
        .arg("--dataset")
        .arg(&corrupt);
    configure_platform_directories(&mut corrupt_command, temporary.path());
    let corrupt_output = corrupt_command.output().expect("inspect corrupt Dataset");
    assert_eq!(corrupt_output.status.code(), Some(1));
    let corrupt_response: serde_json::Value =
        serde_json::from_slice(&corrupt_output.stdout).unwrap();
    assert_eq!(
        corrupt_response["error"]["message"],
        "Dataset inspection failed"
    );
    assert!(
        corrupt_response["error"]["causes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|cause| cause.as_str().unwrap().contains("events"))
    );

    let conflict = Command::new(binary)
        .args(["inspect", "--dataset"])
        .arg(&dataset)
        .args(["--pack-dir", "pack"])
        .output()
        .expect("run conflicting arguments");
    assert_eq!(conflict.status.code(), Some(2));
    assert!(conflict.stdout.is_empty());
}
