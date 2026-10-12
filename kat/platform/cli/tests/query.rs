use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[path = "support/parquet.rs"]
mod parquet_fixture;

#[allow(dead_code)]
#[path = "support/mod.rs"]
mod support;

#[path = "support/test_home.rs"]
mod test_home;

const RUN_ID: &str = "019f6e00-0000-7000-8000-000000000031";
const SESSION_ID: &str = "019f6e00-0000-7000-8000-000000000030";
const OTHER_SESSION_ID: &str = "019f6e00-0000-7000-8000-000000000032";
fn cargo_kat() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kat"))
}

#[test]
fn query_help_describes_bounded_inline_and_ndjson_results() {
    let output = Command::new(cargo_kat())
        .args(["query", "--help"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("bounded small results inline"), "{help}");
    assert!(help.contains("larger results as NDJSON files"), "{help}");
    assert!(help.contains("--run <RUN_ID>"), "{help}");
    assert!(help.contains("--session <SESSION_ID>"), "{help}");
    assert!(help.contains("--sql <SQL>"), "{help}");
    assert!(!help.contains("--dataset"), "{help}");
    assert!(!help.contains("positional JSON scalars"), "{help}");
}

#[test]
fn query_requires_both_session_and_run_id() {
    for arguments in [
        vec!["query", "--run", RUN_ID, "--sql", "SELECT 1"],
        vec!["query", "--session", SESSION_ID, "--sql", "SELECT 1"],
    ] {
        let output = Command::new(cargo_kat()).args(&arguments).output().unwrap();

        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
    }
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
use std::{env, fs, path::Path, process, thread, time::Duration};

#[cfg(windows)]
use std::{
    ffi::c_void,
    os::windows::ffi::OsStrExt,
    ptr,
};

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *mut c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: *mut c_void,
    ) -> *mut c_void;
    fn CloseHandle(object: *mut c_void) -> i32;
}

#[cfg(windows)]
fn hold_delete_lock(path: &Path, ready: &Path, release: &Path) -> ! {
    let wide = path.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            0x8000_0000,
            0x0000_0001 | 0x0000_0002,
            ptr::null_mut(),
            3,
            0x0000_0080,
            ptr::null_mut(),
        )
    };
    if handle as isize == -1 {
        process::exit(97);
    }
    fs::write(ready, "ready").unwrap();
    while !release.exists() {
        thread::sleep(Duration::from_millis(10));
    }
    unsafe {
        CloseHandle(handle);
    }
    process::exit(0)
}

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
    #[cfg(windows)]
    if arguments.len() == 4 && arguments[0] == "--hold-delete-lock" {
        hold_delete_lock(
            Path::new(&arguments[1]),
            Path::new(&arguments[2]),
            Path::new(&arguments[3]),
        );
    }
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
        Ok("undeletable") => fs::write(&result_path, "{\"value\":1}\n").unwrap(),
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
    let response = match env::var_os("KAT_FAKE_RUNTIME_RESPONSE_FILE") {
        Some(path) => fs::read(path).unwrap(),
        None => env::var("KAT_FAKE_RUNTIME_RESPONSE").unwrap().into_bytes(),
    };
    fs::write(&arguments[10], response).unwrap();
    if let Some(response_written) = env::var_os("KAT_FAKE_RESPONSE_WRITTEN") {
        fs::write(response_written, "written").unwrap();
        let release = env::var("KAT_FAKE_RUNTIME_RELEASE").unwrap();
        while !Path::new(&release).exists() {
            thread::sleep(Duration::from_millis(10));
        }
    }
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
    let sessions = data_home(root).join("sessions");
    let session = sessions.join(SESSION_ID);
    fs::create_dir_all(sessions.join(".leases")).unwrap();
    fs::create_dir_all(sessions.join(".deletions")).unwrap();
    fs::write(
        sessions.join(".leases").join(format!("{SESSION_ID}.lock")),
        [],
    )
    .unwrap();
    fs::create_dir_all(session.join("materializations")).unwrap();
    fs::create_dir_all(session.join("scratch")).unwrap();
    let run = session.join("runs").join(RUN_ID);
    fs::create_dir_all(run.join("outputs")).unwrap();
    fs::write(
        session.join("session.json"),
        serde_json::to_vec(&serde_json::json!({"session_id": SESSION_ID})).unwrap(),
    )
    .unwrap();
    parquet_fixture::write_i64(&run.join("outputs/main.parquet"), "value", &[1]);
    let mut manifest = serde_json::json!({
        "session_id": SESSION_ID,
        "run_id": RUN_ID,
        "pack": "alpha",
        "workflow": "analyze",
        "guide": null,
        "child_runs": [],
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
        .args([
            "--session",
            SESSION_ID,
            "--run",
            RUN_ID,
            "--sql",
            sql,
        ])
        .env("KAT_CAPTURE_REQUEST", captured)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"format":"json","columns":[{"name":"value","type":"int64"},{"name":"amount","type":"decimal128(10, 3)"}],"rows":[{"value":1,"precise":12345678901234567890.123456789012345678}]}}"#,
        );
    command
}

fn real_query(binary: &Path, root: &Path, sql: &str) -> serde_json::Value {
    let mut command = Command::new(binary);
    test_home::configure(&mut command, root);
    let output = command
        .arg("query")
        .args(["--session", SESSION_ID, "--run", RUN_ID, "--sql", sql])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
#[ignore = "requires KAT_TEST_PYTHON and a wheel built from the current checkout"]
fn real_host_query_covers_inline_and_file_boundaries() {
    let python = PathBuf::from(
        std::env::var_os("KAT_TEST_PYTHON").expect("KAT_TEST_PYTHON identifies CPython"),
    );
    let workflow_wheel = PathBuf::from(
        std::env::var_os("KAT_TEST_WORKFLOW_WHEEL")
            .expect("KAT_TEST_WORKFLOW_WHEEL identifies the current wheel"),
    );
    let temporary = tempfile::tempdir().unwrap();
    let (_skill, binary) =
        support::stage_real_host_skill(temporary.path(), &cargo_kat(), &python, &workflow_wheel);
    let run = write_manifest(temporary.path(), None);
    let values = (0..101).collect::<Vec<_>>();
    parquet_fixture::write_i64(&run.join("outputs/main.parquet"), "value", &values);
    let manifest_path = run.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["outputs"]["main"]["row_count"] = 101.into();
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let empty = real_query(
        &binary,
        temporary.path(),
        "SELECT value FROM output.main WHERE FALSE",
    );
    assert_eq!(empty["result"]["format"], "json");
    assert_eq!(empty["result"]["rows"], serde_json::json!([]));
    assert_eq!(empty["result"]["columns"][0]["name"], "value");
    assert_eq!(empty["result"].as_object().unwrap().len(), 3);

    let hundred = real_query(
        &binary,
        temporary.path(),
        "SELECT value FROM output.main ORDER BY value LIMIT 100",
    );
    assert_eq!(hundred["result"]["format"], "json");
    assert_eq!(hundred["result"]["rows"].as_array().unwrap().len(), 100);
    assert!(hundred["result"].get("path").is_none());

    let exact_bytes = real_query(
        &binary,
        temporary.path(),
        "SELECT repeat('x', 65523) AS value FROM output.main LIMIT 1",
    );
    assert_eq!(exact_bytes["result"]["format"], "json");
    assert_eq!(
        exact_bytes["result"]["rows"][0]["value"]
            .as_str()
            .unwrap()
            .len(),
        65_523
    );
    let result_directory = data_home(temporary.path()).join("query-results");
    assert_eq!(fs::read_dir(&result_directory).unwrap().count(), 0);

    let hundred_one = real_query(
        &binary,
        temporary.path(),
        "SELECT value FROM output.main ORDER BY value",
    );
    assert_eq!(hundred_one["result"]["format"], "ndjson");
    assert!(hundred_one["result"].get("rows").is_none());
    assert_eq!(hundred_one["result"].as_object().unwrap().len(), 3);
    let hundred_one_path = Path::new(hundred_one["result"]["path"].as_str().unwrap());
    assert_eq!(
        fs::read_to_string(hundred_one_path)
            .unwrap()
            .lines()
            .count(),
        101
    );

    let over_bytes = real_query(
        &binary,
        temporary.path(),
        "SELECT repeat('x', 65524) AS value FROM output.main LIMIT 1",
    );
    assert_eq!(over_bytes["result"]["format"], "ndjson");
    let over_bytes_path = Path::new(over_bytes["result"]["path"].as_str().unwrap());
    assert_eq!(fs::metadata(over_bytes_path).unwrap().len(), 65_537);
}

#[test]
fn query_reads_final_manifest_and_sends_only_runtime_inputs() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = stage_skill(temporary.path());
    let run = write_manifest(
        temporary.path(),
        Some(serde_json::json!({"legacy": ["invalid", 42]})),
    );
    let legacy_run = data_home(temporary.path())
        .join("runs")
        .join(RUN_ID)
        .join("manifest.json");
    let legacy_datasource = data_home(temporary.path()).join("datasources/alpha/sentinel");
    fs::create_dir_all(legacy_run.parent().unwrap()).unwrap();
    fs::create_dir_all(legacy_datasource.parent().unwrap()).unwrap();
    fs::write(&legacy_run, b"legacy root must not be read").unwrap();
    fs::write(&legacy_datasource, b"legacy datasource").unwrap();
    let manifest_path = run.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["outputs"]["summary"] = serde_json::json!({
        "columns": [{"name":"value","type":"int64"}],
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
    let output_text = String::from_utf8(output.stdout.clone()).unwrap();
    assert!(
        output_text.contains("12345678901234567890.123456789012345678"),
        "{output_text}"
    );
    let mut response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    response["result"]["rows"][0]
        .as_object_mut()
        .unwrap()
        .remove("precise");
    assert_eq!(
        response["result"],
        serde_json::json!({
            "format":"json",
            "columns":[
                {"name":"value","type":"int64"},
                {"name":"amount","type":"decimal128(10, 3)"}
            ],
            "rows":[{"value":1}]
        })
    );
    let request: serde_json::Value = serde_json::from_slice(&fs::read(&captured).unwrap()).unwrap();
    let result_path = Path::new(request["result_path"].as_str().unwrap());
    assert!(!result_path.exists());
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
    assert!(response["result"].get("path").is_none());
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
    assert_ne!(second_response["log_path"], response["log_path"]);
    let second_request: serde_json::Value =
        serde_json::from_slice(&fs::read(second_capture).unwrap()).unwrap();
    assert_ne!(second_request["result_path"], request["result_path"]);
    assert!(!Path::new(second_request["result_path"].as_str().unwrap()).exists());
    assert_eq!(
        fs::read(legacy_run).unwrap(),
        b"legacy root must not be read"
    );
    assert_eq!(fs::read(legacy_datasource).unwrap(), b"legacy datasource");
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
        assert_eq!(response["result"]["format"], "json");
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

fn wait_until_exists(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn inline_result_is_not_published_when_its_candidate_cannot_be_removed() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = stage_skill(temporary.path());
    write_manifest(temporary.path(), None);
    let captured = temporary.path().join("request.json");
    let response_written = temporary.path().join("response-written");
    let runtime_release = temporary.path().join("runtime-release");
    #[cfg(windows)]
    let lock_ready = temporary.path().join("delete-lock-ready");
    #[cfg(windows)]
    let lock_release = temporary.path().join("delete-lock-release");
    let stdout_path = temporary.path().join("query-stdout.json");
    let stderr_path = temporary.path().join("query-stderr.log");
    let mut query = command(
        &binary,
        temporary.path(),
        &captured,
        "SELECT * FROM output.main",
    );
    query
        .env("KAT_FAKE_RESULT_MODE", "undeletable")
        .env("KAT_FAKE_RESPONSE_WRITTEN", &response_written)
        .env("KAT_FAKE_RUNTIME_RELEASE", &runtime_release)
        .stdout(Stdio::from(fs::File::create(&stdout_path).unwrap()))
        .stderr(Stdio::from(fs::File::create(&stderr_path).unwrap()));
    let mut query = query.spawn().unwrap();
    wait_until_exists(&response_written);

    let request: serde_json::Value = serde_json::from_slice(&fs::read(&captured).unwrap()).unwrap();
    let candidate = Path::new(request["result_path"].as_str().unwrap());

    #[cfg(windows)]
    let mut delete_lock = {
        let host = binary.parent().unwrap().join("python/python.exe");
        let child = Command::new(host)
            .args([
                "--hold-delete-lock",
                candidate.to_str().unwrap(),
                lock_ready.to_str().unwrap(),
                lock_release.to_str().unwrap(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        wait_until_exists(&lock_ready);
        child
    };
    #[cfg(unix)]
    fs::set_permissions(
        candidate.parent().unwrap(),
        fs::Permissions::from_mode(0o555),
    )
    .unwrap();

    fs::write(&runtime_release, "release").unwrap();
    let status = query.wait().unwrap();

    #[cfg(windows)]
    {
        fs::write(&lock_release, "release").unwrap();
        assert!(delete_lock.wait().unwrap().success());
    }
    #[cfg(unix)]
    fs::set_permissions(
        candidate.parent().unwrap(),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    assert_eq!(
        status.code(),
        Some(1),
        "{}",
        fs::read_to_string(&stderr_path).unwrap()
    );
    let response: serde_json::Value =
        serde_json::from_slice(&fs::read(&stdout_path).unwrap()).unwrap();
    assert_eq!(
        response["error"]["message"],
        "Inline Query Result candidate could not be removed"
    );
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert!(!log.contains(r#"\"value\":1"#));
    fs::remove_file(candidate).unwrap();
}

#[test]
fn query_never_scans_another_session_for_the_run_id() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = stage_skill(temporary.path());
    write_manifest(temporary.path(), None);
    let sessions = data_home(temporary.path()).join("sessions");
    let other = sessions.join(OTHER_SESSION_ID);
    fs::write(
        sessions
            .join(".leases")
            .join(format!("{OTHER_SESSION_ID}.lock")),
        [],
    )
    .unwrap();
    fs::create_dir_all(other.join("materializations")).unwrap();
    fs::create_dir_all(other.join("scratch")).unwrap();
    fs::create_dir_all(other.join("runs")).unwrap();
    fs::write(
        other.join("session.json"),
        serde_json::to_vec(&serde_json::json!({"session_id": OTHER_SESSION_ID})).unwrap(),
    )
    .unwrap();
    let captured = temporary.path().join("unexpected-request.json");
    let mut query = Command::new(&binary);
    test_home::configure(&mut query, temporary.path());
    query
        .arg("query")
        .args([
            "--session",
            OTHER_SESSION_ID,
            "--run",
            RUN_ID,
            "--sql",
            "SELECT * FROM output.main",
        ])
        .env("KAT_CAPTURE_REQUEST", &captured);

    let output = query.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        format!("Run {RUN_ID} does not exist in Analysis Session {OTHER_SESSION_ID}")
    );
    assert!(!captured.exists());
}

#[test]
fn active_query_blocks_session_delete_until_the_query_response_is_published() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = stage_skill(temporary.path());
    write_manifest(temporary.path(), None);
    let captured = temporary.path().join("request.json");
    let response_written = temporary.path().join("response-written");
    let runtime_release = temporary.path().join("runtime-release");
    let mut query = command(
        &binary,
        temporary.path(),
        &captured,
        "SELECT * FROM output.main",
    );
    query
        .env("KAT_FAKE_RESPONSE_WRITTEN", &response_written)
        .env("KAT_FAKE_RUNTIME_RELEASE", &runtime_release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let query = query.spawn().unwrap();
    wait_until_exists(&response_written);

    let mut deleting = Command::new(&binary);
    test_home::configure(&mut deleting, temporary.path());
    let blocked = deleting
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();
    fs::write(&runtime_release, "release").unwrap();
    let query = query.wait_with_output().unwrap();

    assert_eq!(blocked.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&blocked.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        format!("Analysis Session {SESSION_ID} is in use")
    );
    assert_eq!(
        query.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&query.stderr)
    );
    assert!(
        data_home(temporary.path())
            .join("sessions")
            .join(SESSION_ID)
            .is_dir()
    );
}

#[test]
fn query_holds_session_lease_until_its_large_response_is_flushed() {
    let temporary = tempfile::tempdir().unwrap();
    let binary = stage_skill(temporary.path());
    write_manifest(temporary.path(), None);
    let session = data_home(temporary.path())
        .join("sessions")
        .join(SESSION_ID);
    let captured = temporary.path().join("request.json");
    let runtime_response = temporary.path().join("large-runtime-response.json");
    fs::write(
        &runtime_response,
        serde_json::to_vec(&serde_json::json!({
            "status": "success",
            "result": {
                "format": "ndjson",
                "columns": [{"name": "x".repeat(8 * 1024 * 1024), "type": "int64"}]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let mut query = command(
        &binary,
        temporary.path(),
        &captured,
        "SELECT * FROM output.main",
    );
    query
        .env("KAT_FAKE_RUNTIME_RESPONSE_FILE", &runtime_response)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut query = query.spawn().unwrap();
    let mut stdout = query.stdout.take().unwrap();
    let mut first_byte = [0];
    stdout.read_exact(&mut first_byte).unwrap();
    assert_eq!(first_byte, [b'{']);

    let mut deleting = Command::new(&binary);
    test_home::configure(&mut deleting, temporary.path());
    deleting
        .args(["session", "delete", "--session", SESSION_ID])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut deleting = deleting.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let delete_completed = loop {
        if deleting.try_wait().unwrap().is_some() {
            break true;
        }
        if Instant::now() >= deadline {
            deleting.kill().unwrap();
            break false;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let blocked = deleting.wait_with_output().unwrap();
    let session_remained_while_blocked = session.is_dir();
    let tombstone_remained_absent = !data_home(temporary.path())
        .join("sessions/.deletions")
        .join(SESSION_ID)
        .exists();

    let mut frame = Vec::with_capacity(8 * 1024 * 1024);
    frame.extend_from_slice(&first_byte);
    stdout.read_to_end(&mut frame).unwrap();
    let status = query.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    let response: serde_json::Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(
        response["result"]["columns"][0]["name"]
            .as_str()
            .unwrap()
            .len(),
        8 * 1024 * 1024
    );

    let mut deleting = Command::new(&binary);
    test_home::configure(&mut deleting, temporary.path());
    let deleted = deleting
        .args(["session", "delete", "--session", SESSION_ID])
        .output()
        .unwrap();

    assert!(
        delete_completed,
        "Session delete did not fail immediately while Query stdout was blocked"
    );
    assert_eq!(blocked.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&blocked.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        format!("Analysis Session {SESSION_ID} is in use")
    );
    assert!(session_remained_while_blocked);
    assert!(tombstone_remained_absent);
    assert_eq!(
        deleted.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&deleted.stderr)
    );
    assert!(!session.exists());
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
            r#"{"status":"success","result":{"format":"json","columns":[]}}"#,
            false,
        ),
        (
            "positional rows",
            None,
            r#"{"status":"success","result":{"format":"json","columns":[],"rows":[[1]]}}"#,
            false,
        ),
        (
            "rows in file result",
            None,
            r#"{"status":"success","result":{"format":"ndjson","columns":[],"rows":[]}}"#,
            false,
        ),
        (
            "missing file",
            Some("none"),
            r#"{"status":"success","result":{"format":"ndjson","columns":[]}}"#,
            false,
        ),
        (
            "directory instead of file",
            Some("directory"),
            r#"{"status":"success","result":{"format":"ndjson","columns":[]}}"#,
            false,
        ),
        (
            "host exit",
            None,
            r#"{"status":"success","result":{"format":"ndjson","columns":[]}}"#,
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
        let log_path = Path::new(response["log_path"].as_str().unwrap());
        assert!(log_path.is_file(), "{name} lost its Operation log");
        let log = fs::read_to_string(log_path).unwrap();
        assert!(!log.contains(r#"\"value\":1"#), "{name} logged Query rows");
    }
}
