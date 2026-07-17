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
    let source = payload.join("fake-test-host.rs");
    fs::write(
        &source,
        r#"
use std::{env, fs, io::{self, Write}, process};

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let fixed = ["-I", "-B", "-X", "utf8", "-u", "-m", "_kat_runtime", "--request"];
    if arguments.len() != 13
        || arguments[..8] != fixed
        || arguments[9] != "--response"
        || arguments[11] != "--test-report"
    {
        process::exit(91);
    }
    let request = fs::read_to_string(&arguments[8]).unwrap();
    if !request.contains("\"operation\":\"test_pack\"")
        || request.contains("test_report")
    {
        process::exit(92);
    }
    fs::write(env::var("KAT_CAPTURE_REQUEST").unwrap(), &request).unwrap();
    fs::write(
        env::var("KAT_CAPTURE_CWD").unwrap(),
        env::current_dir().unwrap().to_string_lossy().as_bytes(),
    ).unwrap();
    io::stdout().write_all(b"\x1b[31mpytest stdout\x1b[0m\r\n").unwrap();
    io::stderr().write_all(b"tests/test_flow.py::test_case FAILED\r\n").unwrap();
    if env::var_os("KAT_SKIP_REPORT").is_none() {
        fs::write(&arguments[12], "<testsuites />\n").unwrap();
    }
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

fn write_pack(directory: &Path, name: &str) {
    fs::create_dir_all(directory.join("tests/datasets/sample/tables")).unwrap();
    fs::write(
        directory.join("pack.toml"),
        format!(
            "name = {name:?}\ntitle = {name:?}\ndescription = \"Test fixture\"\nowner = \"Test\"\n"
        ),
    )
    .unwrap();
    fs::write(
        directory.join("tests/test_flow.py"),
        "def test_case(): pass\n",
    )
    .unwrap();
    fs::write(directory.join("tests/datasets/sample/.kat-dataset"), []).unwrap();
    fs::write(
        directory.join("tests/datasets/sample/tables/data_dict.parquet"),
        base64::engine::general_purpose::STANDARD
            .decode(PARQUET)
            .unwrap(),
    )
    .unwrap();
}

fn test_command(binary: &Path, root: &Path, pack: &Path) -> Command {
    let captured = root.join("captured-request.json");
    let cwd = root.join("unrelated-cwd");
    fs::create_dir_all(&cwd).unwrap();
    let mut command = Command::new(binary);
    command
        .current_dir(&cwd)
        .arg("test")
        .args(["--pack", "alpha"])
        .arg("--pack-dir")
        .arg(pack)
        .args(["--test", "tests/test_flow.py::test_case[case::value]"])
        .env("KAT_CAPTURE_REQUEST", captured)
        .env("KAT_CAPTURE_CWD", root.join("captured-cwd.txt"))
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"summary":{"passed":2}}}"#,
        );
    configure(&mut command, root);
    command
}

#[test]
fn test_success_covers_external_and_bundled_packs_from_an_arbitrary_cwd() {
    for bundled in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let (skill, binary) = stage_skill(temporary.path());
        stage_fake_host(&binary);
        let pack = if bundled {
            skill.join("assets/packs/alpha-source")
        } else {
            temporary.path().join("external-pack")
        };
        write_pack(&pack, "alpha");

        let output = test_command(&binary, temporary.path(), &pack)
            .output()
            .unwrap();

        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["status"], "success");
        assert_eq!(
            response["result"],
            serde_json::json!({"summary":{"passed":2}})
        );
        let report = PathBuf::from(response["test_report_path"].as_str().unwrap());
        let log = PathBuf::from(response["log_path"].as_str().unwrap());
        assert_eq!(report.file_stem(), log.file_stem());
        assert_eq!(fs::read_to_string(&report).unwrap(), "<testsuites />\n");
        let projected = String::from_utf8(output.stderr).unwrap();
        assert!(projected.contains("pytest stdout\n"));
        assert!(projected.contains("tests/test_flow.py::test_case FAILED\n"));
        assert!(fs::read_to_string(log).unwrap().contains(&projected));

        let request: serde_json::Value = serde_json::from_slice(
            &fs::read(temporary.path().join("captured-request.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(request["pack_name"], "alpha");
        assert_eq!(
            request["pack_path"],
            dunce::canonicalize(&pack).unwrap().to_str().unwrap()
        );
        assert_eq!(
            request["tests"],
            serde_json::json!(["tests/test_flow.py::test_case[case::value]"])
        );
        assert!(request.get("test_report").is_none());
        assert!(
            request["datasets"]["sample"]["tables"]["data_dict"]
                .as_str()
                .unwrap()
                .ends_with("data_dict.parquet")
        );
        assert_eq!(
            dunce::canonicalize(
                fs::read_to_string(temporary.path().join("captured-cwd.txt")).unwrap()
            )
            .unwrap(),
            dunce::canonicalize(&pack).unwrap()
        );
    }
}

#[test]
fn test_preflight_rejects_missing_tests_invalid_selectors_and_invalid_datasets() {
    let cases = [
        "missing-tests",
        "invalid-selector",
        "invalid-dataset",
        "invalid-marker",
    ];
    for case in cases {
        let temporary = tempfile::tempdir().unwrap();
        let (_skill, binary) = stage_skill(temporary.path());
        stage_fake_host(&binary);
        let pack = temporary.path().join("pack");
        write_pack(&pack, "alpha");
        match case {
            "missing-tests" => fs::remove_dir_all(pack.join("tests")).unwrap(),
            "invalid-dataset" => {
                fs::write(
                    pack.join("tests/datasets/sample/tables/data_dict.parquet"),
                    "broken",
                )
                .unwrap();
            }
            "invalid-marker" => {
                fs::remove_file(pack.join("tests/datasets/sample/.kat-dataset")).unwrap();
                fs::create_dir(pack.join("tests/datasets/sample/.kat-dataset")).unwrap();
            }
            _ => {}
        }
        let captured = temporary.path().join("unexpected-request.json");
        let mut command = Command::new(&binary);
        command
            .arg("test")
            .args(["--pack", "alpha"])
            .arg("--pack-dir")
            .arg(&pack)
            .env("KAT_CAPTURE_REQUEST", &captured)
            .env("KAT_CAPTURE_CWD", temporary.path().join("cwd.txt"))
            .env("KAT_FAKE_RUNTIME_RESPONSE", "unused");
        if case == "invalid-selector" {
            command.args(["--test", "../tests/test_flow.py"]);
        }
        configure(&mut command, temporary.path());

        let output = command.output().unwrap();

        assert_eq!(output.status.code(), Some(1), "{case}");
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["status"], "failure");
        assert!(response.get("result").is_none());
        assert!(response.get("test_report_path").is_none());
        assert!(!captured.exists());
        assert!(Path::new(response["log_path"].as_str().unwrap()).is_file());
    }
}

#[test]
fn test_runtime_failures_and_missing_reports_keep_the_final_failure_owner() {
    for (runtime_success, write_report) in [(false, true), (false, false), (true, false)] {
        let temporary = tempfile::tempdir().unwrap();
        let (_skill, binary) = stage_skill(temporary.path());
        stage_fake_host(&binary);
        let pack = temporary.path().join("pack");
        write_pack(&pack, "alpha");
        let captured = temporary.path().join("request.json");
        let mut command = Command::new(&binary);
        command
            .arg("test")
            .args(["--pack", "alpha"])
            .arg("--pack-dir")
            .arg(&pack)
            .env("KAT_CAPTURE_REQUEST", &captured)
            .env("KAT_CAPTURE_CWD", temporary.path().join("cwd.txt"))
            .env(
                "KAT_FAKE_RUNTIME_RESPONSE",
                if runtime_success {
                    r#"{"status":"success","result":{"summary":{"passed":1}}}"#
                } else {
                    r#"{"status":"failure","error":{"message":"PACK tests failed","help":"Inspect pytest"}}"#
                },
            );
        if !write_report {
            command.env("KAT_SKIP_REPORT", "1");
        }
        configure(&mut command, temporary.path());

        let output = command.output().unwrap();

        assert_eq!(output.status.code(), Some(1));
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["status"], "failure");
        assert!(response.get("result").is_none());
        if !runtime_success {
            assert_eq!(response["error"]["message"], "PACK tests failed");
        } else {
            assert_eq!(
                response["error"]["message"],
                "pytest succeeded without delivering the PACK Test Report"
            );
        }
        assert_eq!(response.get("test_report_path").is_some(), write_report);
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("tests/test_flow.py::test_case FAILED")
        );
    }
}

#[test]
fn test_operation_log_creation_failure_never_starts_runtime() {
    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let pack = temporary.path().join("pack");
    write_pack(&pack, "alpha");
    fs::create_dir_all(data_home(temporary.path())).unwrap();
    fs::write(data_home(temporary.path()).join("logs"), "not a directory").unwrap();
    let captured = temporary.path().join("unexpected-request.json");
    let mut command = Command::new(&binary);
    command
        .arg("test")
        .args(["--pack", "alpha"])
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env("KAT_CAPTURE_CWD", temporary.path().join("cwd.txt"))
        .env("KAT_FAKE_RUNTIME_RESPONSE", "unused");
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert!(response.get("log_path").is_none());
    assert!(response.get("test_report_path").is_none());
    assert!(!captured.exists());
}
