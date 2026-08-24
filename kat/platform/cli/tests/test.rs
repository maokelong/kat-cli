use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use base64::Engine;

#[allow(dead_code)]
mod support;
#[path = "support/test_home.rs"]
mod test_home;

const PARQUET: &str = "UEFSMRUEFSAVIEwVBBUAEgAAAQAAAAAAAAACAAAAAAAAABUAFRIVEiwVBBUQFQYVBgAAAgAAAAQBAQMCFQQVMBUwTBUEFQASAAAGAAAAY2FsbGVyCgAAAGZ1dGV4X3dhaXQVABUSFRIsFQQVEBUGFQYAAAIAAAAEAQEDAhkSAhkYCAEAAAAAAAAAGRgIAgAAAAAAAAAVAhkWACkmAAQAGRICGRgGY2FsbGVyGRgKZnV0ZXhfd2FpdBUCGRYAKSYABAAZHBZEFTQWAAAAGRwWxAEVNBYAABkWIAAVAhk8SAxhcnJvd19zY2hlbWEVBAAVBCUCGAJpZAAVDCUCGARkYXRhJQBMHAAAABYEGRwZLCYAHBUEGTUABhAZGAJpZBUAFgQWcBZwJkQmCBwYCAIAAAAAAAAAGAgBAAAAAAAAABYAKAgCAAAAAAAAABgIAQAAAAAAAAAREQAZLBUEFQAVAgAVABUQFQIAPDkmAAQAABaEAxUUFvgBFUYAJgAcFQwZNQAGEBkYBGRhdGEVABYEFoABFoABJsQBJngcNgAoCmZ1dGV4X3dhaXQYBmNhbGxlchERABksFQQVABUCABUAFRAVAgA8FiApJgAEAAAWmAMVHBa+AhVGABbwARYEJggW8AEUAAAZHBgMQVJST1c6c2NoZW1hGOwBLy8vLy82Z0FBQUFRQUFBQUFBQUtBQXdBQ2dBSkFBUUFDZ0FBQUJBQUFBQUFBUVFBQ0FBSUFBQUFCQUFJQUFBQUJBQUFBQUlBQUFCRUFBQUFCQUFBQU5ULy8vOFlBQUFBREFBQUFBQUFBUVVRQUFBQUFBQUFBQVFBQkFBRUFBQUFCQUFBQUdSaGRHRUFBQUFBRUFBVUFCQUFEZ0FQQUFRQUFBQUlBQkFBQUFBWUFBQUFJQUFBQUFBQUFRSWNBQUFBQ0FBTUFBUUFDd0FJQUFBQVFBQUFBQUFBQUFFQUFBQUFBZ0FBQUdsa0FBQT0AGBlwYXJxdWV0LXJzIHZlcnNpb24gNTguMy4wGSwcAAAcAAAALwIAAFBBUjE=";

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
use std::{
    env,
    fs,
    io::{self, Write},
    path::PathBuf,
    process,
    thread,
    time::Duration,
};

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
    io::stdout().flush().unwrap();
    io::stderr().flush().unwrap();
    if let Some(release) = env::var_os("KAT_RELEASE_FILE") {
        let release = PathBuf::from(release);
        for _ in 0..3_000 {
            if release.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        if !release.exists() {
            process::exit(93);
        }
    }
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

fn write_pack(directory: &Path, name: &str) {
    fs::create_dir_all(directory.join("tests")).unwrap();
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
}

fn write_sample_dataset(directory: &Path) {
    let dataset = directory.join("tests/datasets/sample");
    fs::create_dir_all(dataset.join("sources/alpha/facts/tables")).unwrap();
    fs::write(dataset.join(".kat-dataset"), []).unwrap();
    fs::write(
        dataset.join("bindings.json"),
        serde_json::to_vec(&serde_json::json!({
            "bindings": [{
                "pack": "alpha",
                "source": "facts",
                "kind": "materialized",
                "arguments": [],
                "working_directory": dunce::canonicalize(directory).unwrap(),
                "tables": ["data_dict"],
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        dataset.join("sources/alpha/facts/tables/data_dict.parquet"),
        base64::engine::general_purpose::STANDARD
            .decode(PARQUET)
            .unwrap(),
    )
    .unwrap();
}

fn fake_host_test_command(binary: &Path, root: &Path, pack: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .arg("test")
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", root.join("captured-request.json"))
        .env("KAT_CAPTURE_CWD", root.join("captured-cwd.txt"));
    test_home::configure(&mut command, root);
    command
}

#[test]
fn test_success_uses_the_explicit_target_pack_from_an_arbitrary_cwd() {
    for bundled in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let (skill, binary) = support::stage_skill(temporary.path(), "skill");
        stage_fake_host(&binary);
        let pack = if bundled {
            skill.join("assets/packs/alpha-source")
        } else {
            temporary.path().join("external-pack")
        };
        write_pack(&pack, "alpha");
        write_sample_dataset(&pack);
        let unrelated_broken_pack = skill.join("assets/packs/unrelated-broken");
        fs::create_dir_all(&unrelated_broken_pack).unwrap();
        fs::write(
            unrelated_broken_pack.join("pack.toml"),
            "not valid TOML = [",
        )
        .unwrap();
        let unrelated_duplicate_pack =
            test_home::data_home(temporary.path()).join("packs/unrelated-duplicate");
        write_pack(&unrelated_duplicate_pack, "alpha");

        let cwd = temporary.path().join("unrelated-cwd");
        fs::create_dir_all(&cwd).unwrap();
        let mut command = fake_host_test_command(&binary, temporary.path(), &pack);
        command
            .current_dir(cwd)
            .args(["--test", "tests/test_flow.py::test_case[case::value]"])
            .env(
                "KAT_FAKE_RUNTIME_RESPONSE",
                r#"{"status":"success","result":{"summary":{"passed":2}}}"#,
            );
        let output = command.output().unwrap();

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
        assert_eq!(request["datasets"]["sample"]["sources"][0]["pack"], "alpha");
        assert_eq!(
            request["datasets"]["sample"]["sources"][0]["source"],
            "facts"
        );
        assert_eq!(
            request["datasets"]["sample"]["sources"][0]["tables"][0]["name"],
            "data_dict"
        );
        assert!(
            request["datasets"]["sample"]["sources"][0]["tables"][0]["path"]
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
fn test_streams_runtime_output_before_runtime_exits() {
    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = support::stage_skill(temporary.path(), "skill");
    stage_fake_host(&binary);
    let pack = temporary.path().join("pack");
    write_pack(&pack, "alpha");
    let release = temporary.path().join("release-runtime");
    let mut command = fake_host_test_command(&binary, temporary.path(), &pack);
    command
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"summary":{"passed":1}}}"#,
        )
        .env("KAT_RELEASE_FILE", &release)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut first_line = String::new();
        reader.read_line(&mut first_line).unwrap();
        let _ = sender.send(first_line.clone());
        let mut remaining = Vec::new();
        reader.read_to_end(&mut remaining).unwrap();
        (first_line.into_bytes(), remaining)
    });

    let first_line = match receiver.recv_timeout(Duration::from_secs(10)) {
        Ok(line) => line,
        Err(error) => {
            fs::write(&release, []).unwrap();
            let _ = child.wait();
            let _ = reader.join();
            panic!("Runtime output was not relayed while the Runtime was running: {error}");
        }
    };
    assert!(
        first_line.contains("pytest stdout")
            || first_line.contains("tests/test_flow.py::test_case FAILED"),
        "unexpected first line: {first_line:?}"
    );
    assert!(
        child.try_wait().unwrap().is_none(),
        "CLI exited before the Runtime was released"
    );

    fs::write(&release, []).unwrap();
    assert!(child.wait().unwrap().success());
    let (mut stderr, remaining) = reader.join().unwrap();
    stderr.extend_from_slice(&remaining);
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(stderr.contains("pytest stdout"));
    assert!(stderr.contains("tests/test_flow.py::test_case FAILED"));
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
        let (_skill, binary) = support::stage_skill(temporary.path(), "skill");
        stage_fake_host(&binary);
        let pack = temporary.path().join("pack");
        write_pack(&pack, "alpha");
        if matches!(case, "invalid-dataset" | "invalid-marker") {
            write_sample_dataset(&pack);
        }
        match case {
            "missing-tests" => fs::remove_dir_all(pack.join("tests")).unwrap(),
            "invalid-dataset" => {
                fs::write(
                    pack.join("tests/datasets/sample/sources/alpha/facts/tables/data_dict.parquet"),
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
        let mut command = fake_host_test_command(&binary, temporary.path(), &pack);
        command
            .env("KAT_CAPTURE_REQUEST", &captured)
            .env("KAT_CAPTURE_CWD", temporary.path().join("cwd.txt"))
            .env("KAT_FAKE_RUNTIME_RESPONSE", "unused");
        if case == "invalid-selector" {
            command.args(["--test", "../tests/test_flow.py"]);
        }

        let output = command.output().unwrap();

        assert_eq!(output.status.code(), Some(1), "{case}");
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["status"], "failure");
        assert!(response.get("result").is_none());
        assert!(response.get("test_report_path").is_none());
        if case == "invalid-selector" {
            assert_eq!(
                response["error"]["help"],
                "Use a pytest node ID whose path begins with tests/ and has no parent-directory component"
            );
        }
        if matches!(case, "invalid-dataset" | "invalid-marker") {
            assert_eq!(
                response["error"]["help"],
                "删除无效的 `tests/datasets/<name>` 后，参照 PACK 的 Source Guide，使用 `kat bind` 或 `kat materialize` 重新创建 Test Dataset；具体参数分别见 `kat bind --help` 和 `kat materialize --help`。若该 Dataset 由数据供应方交付，请重新获取可用副本"
            );
        }
        assert!(!captured.exists());
        assert!(Path::new(response["log_path"].as_str().unwrap()).is_file());
    }
}

#[test]
fn test_runtime_failures_and_missing_reports_keep_the_final_failure_owner() {
    for (runtime_success, write_report) in [(false, true), (false, false), (true, false)] {
        let temporary = tempfile::tempdir().unwrap();
        let (_skill, binary) = support::stage_skill(temporary.path(), "skill");
        stage_fake_host(&binary);
        let pack = temporary.path().join("pack");
        write_pack(&pack, "alpha");
        let captured = temporary.path().join("request.json");
        let mut command = fake_host_test_command(&binary, temporary.path(), &pack);
        command
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
fn test_requires_one_target_directory_and_derives_the_manifest_name() {
    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = support::stage_skill(temporary.path(), "skill");
    let pack = temporary.path().join("target-pack");
    write_pack(&pack, "alpha");

    let mut missing_directory = Command::new(&binary);
    missing_directory.arg("test");
    test_home::configure(&mut missing_directory, temporary.path());
    let missing_directory = missing_directory.output().unwrap();
    assert_eq!(missing_directory.status.code(), Some(2));
    assert!(missing_directory.stdout.is_empty());

    let mut repeated_directory = Command::new(&binary);
    repeated_directory
        .arg("test")
        .arg("--pack-dir")
        .arg(&pack)
        .arg("--pack-dir")
        .arg(&pack);
    test_home::configure(&mut repeated_directory, temporary.path());
    let repeated_directory = repeated_directory.output().unwrap();
    assert_eq!(repeated_directory.status.code(), Some(2));
    assert!(repeated_directory.stdout.is_empty());

    stage_fake_host(&binary);
    let captured = temporary.path().join("request.json");
    let mut target_directory = fake_host_test_command(&binary, temporary.path(), &pack);
    target_directory
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env("KAT_CAPTURE_CWD", temporary.path().join("cwd.txt"))
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"summary":{"passed":1}}}"#,
        );
    let output = target_directory.output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let request: serde_json::Value = serde_json::from_slice(&fs::read(captured).unwrap()).unwrap();
    assert_eq!(request["pack_name"], "alpha");
}

#[test]
fn test_invalid_target_directory_never_falls_back_to_discovery_paths() {
    let temporary = tempfile::tempdir().unwrap();
    let (skill, binary) = support::stage_skill(temporary.path(), "skill");
    stage_fake_host(&binary);
    write_pack(&skill.join("assets/packs/fallback"), "alpha");
    let captured = temporary.path().join("unexpected-request.json");
    let missing = temporary.path().join("missing-target");
    let mut command = fake_host_test_command(&binary, temporary.path(), &missing);
    command
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env("KAT_CAPTURE_CWD", temporary.path().join("cwd.txt"))
        .env("KAT_FAKE_RUNTIME_RESPONSE", "unused");

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert_eq!(
        response["error"]["message"],
        "target PACK could not be loaded"
    );
    assert!(!captured.exists());
}

#[test]
fn test_runtime_results_and_diagnostics_are_relayed_without_semantic_revalidation() {
    for (runtime_response, expected_status, expected_result, expected_message) in [
        (
            r#"{"status":"success","result":{"summary":{"":0}}}"#,
            "success",
            Some(serde_json::json!({"summary":{"":0}})),
            None,
        ),
        (
            r#"{"status":"failure","error":{"message":""}}"#,
            "failure",
            None,
            Some(""),
        ),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let (_skill, binary) = support::stage_skill(temporary.path(), "skill");
        stage_fake_host(&binary);
        let pack = temporary.path().join("pack");
        write_pack(&pack, "alpha");
        let mut command = fake_host_test_command(&binary, temporary.path(), &pack);
        command.env("KAT_FAKE_RUNTIME_RESPONSE", runtime_response);

        let output = command.output().unwrap();

        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["status"], expected_status);
        assert_eq!(response.get("result"), expected_result.as_ref());
        assert_eq!(
            response["error"]
                .get("message")
                .and_then(serde_json::Value::as_str),
            expected_message
        );
    }
}

#[test]
fn test_transport_failure_remains_a_cli_infrastructure_failure() {
    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = support::stage_skill(temporary.path(), "skill");
    stage_fake_host(&binary);
    let pack = temporary.path().join("pack");
    write_pack(&pack, "alpha");
    let mut command = fake_host_test_command(&binary, temporary.path(), &pack);
    command.env("KAT_FAKE_RUNTIME_RESPONSE", "not JSON");

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    assert!(Path::new(response["log_path"].as_str().unwrap()).is_file());
}

#[test]
#[ignore = "requires KAT_TEST_PYTHON and a wheel built from the current checkout"]
fn test_uses_real_installed_workflow_host_end_to_end() {
    #[derive(Clone, Copy)]
    enum ExpectedOutcome {
        Success,
        RuntimeFailure,
        TransportFailure,
    }

    let python = PathBuf::from(
        std::env::var_os("KAT_TEST_PYTHON").expect("KAT_TEST_PYTHON identifies CPython"),
    );
    let workflow_wheel = PathBuf::from(
        std::env::var_os("KAT_TEST_WORKFLOW_WHEEL")
            .expect("KAT_TEST_WORKFLOW_WHEEL identifies the current wheel"),
    );
    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = support::stage_real_host_skill(
        temporary.path(),
        &support::cargo_kat(),
        &python,
        &workflow_wheel,
    );
    for (case, workflow_source, test_source, expected) in [
        (
            "success",
            r#"import kat

@kat.workflow(name="analyze", title="Analyze")
def analyze(ctx: kat.Context):
    """Return the fixture table."""
    return ctx.sql("SELECT id FROM alpha.facts.data_dict ORDER BY id")
"#,
            r#"def test_case(kat_run):
    assert kat_run(workflow="analyze", dataset="sample")["main"].num_rows == 2
"#,
            ExpectedOutcome::Success,
        ),
        (
            "runtime-failure",
            r#"import kat

@kat.workflow(name="broken", title="Broken")
def broken(ctx: kat.Context):
    """Raise a deterministic execution failure."""
    raise RuntimeError("sentinel Workflow execution failure")
"#,
            r#"def test_case(kat_run):
    kat_run(workflow="broken")
"#,
            ExpectedOutcome::RuntimeFailure,
        ),
        (
            "transport-failure",
            r#"import os
import kat

@kat.workflow(name="interrupt", title="Interrupt")
def interrupt(ctx: kat.Context):
    """Terminate the Host before it can return a Runtime Response."""
    os._exit(17)
"#,
            r#"def test_case(kat_run):
    kat_run(workflow="interrupt")
"#,
            ExpectedOutcome::TransportFailure,
        ),
    ] {
        let case_root = temporary.path().join(case);
        fs::create_dir_all(&case_root).unwrap();
        let pack = case_root.join("pack");
        write_pack(&pack, "alpha");
        if matches!(expected, ExpectedOutcome::Success) {
            write_sample_dataset(&pack);
        }
        fs::create_dir_all(pack.join("workflows")).unwrap();
        fs::write(pack.join("workflows/workflow.py"), workflow_source).unwrap();
        fs::write(pack.join("tests/test_flow.py"), test_source).unwrap();
        let mut command = Command::new(&binary);
        command.arg("test").arg("--pack-dir").arg(&pack);
        test_home::configure(&mut command, &case_root);

        let output = command.output().unwrap();

        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(Path::new(response["log_path"].as_str().unwrap()).is_file());
        match expected {
            ExpectedOutcome::Success => {
                assert_eq!(output.status.code(), Some(0), "{stderr}");
                assert_eq!(response["status"], "success");
                assert_eq!(
                    response["result"],
                    serde_json::json!({"summary":{"passed":1}})
                );
                assert!(Path::new(response["test_report_path"].as_str().unwrap()).is_file());
            }
            ExpectedOutcome::RuntimeFailure => {
                assert_eq!(output.status.code(), Some(1));
                assert_eq!(response["status"], "failure");
                assert_eq!(response["error"]["message"], "PACK tests failed");
                assert!(response.get("result").is_none());
                assert!(Path::new(response["test_report_path"].as_str().unwrap()).is_file());
                assert!(stderr.contains("KAT Workflow test execution failed"));
                assert!(stderr.contains("sentinel Workflow execution failure"));
            }
            ExpectedOutcome::TransportFailure => {
                assert_eq!(output.status.code(), Some(1));
                assert_eq!(response["status"], "failure");
                assert_eq!(response["error"]["message"], "Workflow Runtime failed");
                assert!(response.get("result").is_none());
                assert!(response.get("test_report_path").is_none());
                assert!(
                    response["error"]["causes"]
                        .to_string()
                        .contains("exited without completing Runtime IPC")
                );
            }
        }
    }
}

#[test]
fn test_operation_log_creation_failure_never_starts_runtime() {
    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) = support::stage_skill(temporary.path(), "skill");
    let pack = temporary.path().join("pack");
    write_pack(&pack, "alpha");
    fs::create_dir_all(test_home::data_home(temporary.path())).unwrap();
    fs::write(
        test_home::data_home(temporary.path()).join("logs"),
        "not a directory",
    )
    .unwrap();
    let captured = temporary.path().join("unexpected-request.json");
    let mut command = Command::new(&binary);
    command
        .arg("test")
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env("KAT_CAPTURE_CWD", temporary.path().join("cwd.txt"))
        .env("KAT_FAKE_RUNTIME_RESPONSE", "unused");
    test_home::configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert!(response.get("log_path").is_none());
    assert!(response.get("test_report_path").is_none());
    assert!(!captured.exists());
}
