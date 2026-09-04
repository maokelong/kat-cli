use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;

mod support;
use support::cargo_kat;

fn stage_skill(root: &Path) -> (PathBuf, PathBuf) {
    support::stage_skill(root, "skill")
}

fn stage_fake_host(binary: &Path) {
    let payload = binary.parent().unwrap();
    let host = support::host_path(binary);
    fs::create_dir_all(host.parent().unwrap()).unwrap();
    let output_template = host.parent().unwrap().join("fake-output.parquet");
    let schema = Arc::new(Schema::new(vec![Field::new(
        "value",
        DataType::Int64,
        true,
    )]));
    ArrowWriter::try_new(fs::File::create(output_template).unwrap(), schema, None)
        .unwrap()
        .close()
        .unwrap();
    let source = payload.join("fake-run-host.rs");
    fs::write(
        &source,
        r#"
 use std::{
     env, fs::{self, OpenOptions},
     io::{self, BufRead, Write},
     path::Path,
    process,
    thread,
    time::Duration,
};

fn json_string(document: &str, key: &str) -> String {
    let marker = format!("\"{key}\":\"");
    let value = document.split_once(&marker).unwrap().1;
    let mut decoded = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            let escape = characters.next().expect("unterminated test JSON escape");
            if escape == 'u' {
                let digits = characters.by_ref().take(4).collect::<String>();
                let code = u16::from_str_radix(&digits, 16)
                    .expect("invalid test JSON unicode escape");
                decoded.push(char::from_u32(u32::from(code))
                    .expect("unsupported test JSON surrogate escape"));
            } else {
                decoded.push(match escape {
                '\\' => '\\',
                '"' => '"',
                '/' => '/',
                'b' => '\u{0008}',
                'f' => '\u{000c}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                _ => panic!("unsupported test JSON escape"),
                });
            }
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
     let operation = json_string(&request, "operation");
     let workflow_name = json_string(&request, "workflow_name");
     if let Some(path) = env::var_os("KAT_CAPTURE_HOST_OPERATIONS") {
         let mut operations = OpenOptions::new()
             .create(true)
             .append(true)
             .open(path)
             .unwrap();
         writeln!(operations, "{operation}:{workflow_name}").unwrap();
     }
     if operation == "inspect_workflow" {
         if let Some(path) = env::var_os("KAT_CAPTURE_PREFLIGHT_ALLOCATIONS") {
             let sessions = Path::new(&env::var("KAT_DATA_HOME").unwrap()).join("sessions");
             let mut runs = 0;
             let mut scratch = 0;
             for session in fs::read_dir(sessions).unwrap() {
                 let session = session.unwrap().path();
                 let run_directory = session.join("runs");
                 let scratch_directory = session.join("scratch");
                 if run_directory.is_dir() && scratch_directory.is_dir() {
                     runs += fs::read_dir(run_directory).unwrap().count();
                     scratch += fs::read_dir(scratch_directory).unwrap().count();
                 }
             }
             fs::write(path, format!("{runs},{scratch}")).unwrap();
         }
         let response = match env::var("KAT_FAKE_NESTED_PREFLIGHT_FAILURE") {
             Ok(message) => format!(
                 "{{\"status\":\"failure\",\"error\":{{\"message\":\"{message}\"}}}}"
             ),
             Err(_) => format!(
                 "{{\"status\":\"success\",\"result\":{{\"workflow\":{{\"name\":\"{workflow_name}\",\"description\":\"Fixture Workflow\",\"parameters\":[],\"guide\":null}}}}}}"
             ),
         };
         fs::write(&arguments[10], response).unwrap();
         return;
     }
    let valid_input_shape = match operation.as_str() {
        "run_workflow" => {
            request.contains("\"arguments\":") && !request.contains("\"inputs\":")
        }
        "run_workflow_with_inputs" => {
            !request.contains("\"arguments\":") && request.contains("\"inputs\":")
        }
        _ => false,
    };
    if !valid_input_shape
        || !request.contains("\"candidate_id\":")
        || !request.contains("\"candidate_path\":")
        || !request.contains("\"scratch_root\":")
    {
        process::exit(92);
    }
     fs::write(env::var("KAT_CAPTURE_REQUEST").unwrap(), &request).unwrap();
     let nested_parent_workflow = env::var("KAT_FAKE_NESTED_PARENT_WORKFLOW").ok();
     let second_nested_parent = env::var("KAT_FAKE_SECOND_NESTED_PARENT_WORKFLOW").ok();
     for (parent, pack_key, workflow_key, count) in [
         (
             nested_parent_workflow.as_deref(),
             "KAT_FAKE_NESTED_PACK",
             "KAT_FAKE_NESTED_WORKFLOW",
             env::var("KAT_FAKE_NESTED_CALL_COUNT")
                 .ok()
                 .and_then(|value| value.parse::<u64>().ok())
                 .unwrap_or(1),
         ),
         (
             second_nested_parent.as_deref(),
             "KAT_FAKE_SECOND_NESTED_PACK",
             "KAT_FAKE_SECOND_NESTED_WORKFLOW",
             1,
         ),
     ] {
         if parent == Some(workflow_name.as_str()) {
          for call_id in 1..=count {
             let nested_request = format!(
                 "{{\"call_id\":{call_id},\"pack_name\":\"{}\",\"workflow_name\":\"{}\",\"inputs\":{{}}}}",
                 env::var(pack_key).unwrap(),
                 env::var(workflow_key).unwrap(),
             );
             println!("{nested_request}");
             io::stdout().flush().unwrap();
             let mut nested_response = String::new();
             io::stdin().lock().read_line(&mut nested_response).unwrap();
             if let Some(path) = env::var_os("KAT_CAPTURE_NESTED_RESPONSE") {
                 fs::write(path, &nested_response).unwrap();
             }
             let caught = env::var("KAT_FAKE_CATCH_NESTED_FAILURE_WORKFLOW")
                 .ok()
                 .as_deref()
                 == Some(workflow_name.as_str());
             if !nested_response.contains("\"status\":\"success\"") && !caught {
                 process::exit(93);
             }
          }
         }
     }
    if env::var_os("KAT_FAKE_MANIFEST_DIRECTORY").is_some() {
        let candidate_path = json_string(&request, "candidate_path");
        fs::create_dir(Path::new(&candidate_path).join("manifest.json")).unwrap();
    }
    if env::var_os("KAT_FAKE_SKIP_OUTPUTS").is_none() {
        let candidate_path = json_string(&request, "candidate_path");
        let outputs = Path::new(&candidate_path).join("outputs");
        fs::create_dir(&outputs).unwrap();
        let output = outputs.join("main.parquet");
        if env::var_os("KAT_FAKE_CORRUPT_OUTPUT").is_some() {
            fs::write(output, b"not parquet").unwrap();
        } else {
            let template = env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .join("fake-output.parquet");
            fs::copy(template, output).unwrap();
        }
    }
    if env::var_os("KAT_FAKE_WRITE_SESSION_MARKER").is_some() {
        let candidate_path = json_string(&request, "candidate_path");
        let session_root = Path::new(&candidate_path).parent().unwrap().parent().unwrap();
        fs::write(session_root.join("session.json"), b"Runtime-owned marker").unwrap();
    }
    if env::var_os("KAT_FAKE_REPLACE_SCRATCH_WITH_FILE").is_some() {
        let scratch_root = json_string(&request, "scratch_root");
        fs::remove_dir(&scratch_root).unwrap();
        fs::write(scratch_root, b"runtime replacement").unwrap();
    }
    if env::var_os("KAT_FAKE_REPLACE_CANDIDATE_WITH_FILE").is_some() {
        let candidate_path = json_string(&request, "candidate_path");
        fs::remove_dir(&candidate_path).unwrap();
        fs::write(candidate_path, b"runtime replacement").unwrap();
    }
     eprintln!("fake Workflow output");
     let mut response = env::var("KAT_FAKE_RUNTIME_RESPONSE").unwrap();
     if env::var_os("KAT_FAKE_PARENT_RUNTIME_FAILURE").is_some()
         && nested_parent_workflow.as_deref()
             == Some(json_string(&request, "workflow_name").as_str())
     {
         response = "{\"status\":\"failure\",\"error\":{\"message\":\"parent Workflow failed\"}}"
             .to_owned();
     }
     if env::var("KAT_FAKE_RUNTIME_FAILURE_WORKFLOW").ok().as_deref()
         == Some(workflow_name.as_str())
     {
         response = "{\"status\":\"failure\",\"error\":{\"message\":\"Workflow failed after its child completed\"}}"
             .to_owned();
     }
     let candidate_path = json_string(&request, "candidate_path");
    let session_id = Path::new(&candidate_path)
        .parent().unwrap()
        .parent().unwrap()
        .file_name().unwrap()
        .to_str().unwrap();
    response = response.replace("__SESSION_ID__", session_id);
    response = response.replace(
        "__CANDIDATE_ID__",
        &json_string(&request, "candidate_id"),
    );
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

fn configure(command: &mut Command, root: &Path) {
    let data_home = root.join("data-home");
    fs::create_dir_all(&data_home).unwrap();
    command.env("KAT_DATA_HOME", data_home);
}

fn create_session(binary: &Path, root: &Path) -> String {
    let mut command = Command::new(binary);
    command.args(["session", "create"]);
    configure(&mut command, root);
    let output = command.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["result"]["session_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn query_rows(
    binary: &Path,
    root: &Path,
    session_id: &str,
    run_id: &str,
    sql: &str,
) -> Vec<serde_json::Value> {
    let mut command = Command::new(binary);
    command
        .arg("query")
        .args(["--session", session_id, "--run", run_id, "--sql", sql]);
    configure(&mut command, root);
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
    fs::read_to_string(response["result"]["path"].as_str().unwrap())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn manifests_by_workflow(
    root: &Path,
    session_id: &str,
) -> BTreeMap<String, (String, serde_json::Value)> {
    let runs = data_home(root)
        .join("sessions")
        .join(session_id)
        .join("runs");
    fs::read_dir(runs)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let run_id = entry.file_name().into_string().unwrap();
            let manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(entry.path().join("manifest.json")).unwrap())
                    .unwrap();
            (
                manifest["workflow"].as_str().unwrap().to_owned(),
                (run_id, manifest),
            )
        })
        .collect()
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

struct PlatformDataHomeGuard {
    #[cfg(windows)]
    path: PathBuf,
}

impl PlatformDataHomeGuard {
    fn new(root: &Path) -> Self {
        #[cfg(not(windows))]
        {
            let _ = root;
            Self {}
        }
        #[cfg(windows)]
        {
            let path = data_home(root);
            assert!(
                !path.exists(),
                "Windows Run tests require a clean runner without {path:?}"
            );
            Self { path }
        }
    }
}

impl Drop for PlatformDataHomeGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.path.exists() {
            fs::remove_dir_all(&self.path).expect("remove isolated Windows KAT Data Home");
        }
    }
}

fn data_home(root: &Path) -> PathBuf {
    root.join("data-home")
}

fn pack_named(root: &Path, directory: &str, name: &str) -> PathBuf {
    let pack = root.join(directory);
    fs::create_dir_all(&pack).unwrap();
    fs::write(
        pack.join("pack.toml"),
        format!(
            "name = \"{name}\"\ntitle = \"{name}\"\ndescription = \"Run fixture\"\nowner = \"Test\"\n"
        ),
    )
    .unwrap();
    pack
}

fn pack(root: &Path) -> PathBuf {
    pack_named(root, "pack", "alpha")
}

#[test]
fn run_help_and_parser_do_not_expose_dataset() {
    let output = Command::new(cargo_kat())
        .args(["run", "--help"])
        .output()
        .expect("run operation help");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(help.contains("Operation log may retain the resolved PACK"));
    assert!(help.contains("all arguments after"));
    assert!(help.contains("Do not pass secrets"));
    assert!(!help.contains("Dataset"));
    assert!(!help.contains("--dataset"));

    let removed = Command::new(cargo_kat())
        .args([
            "run",
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
            "--dataset",
            "legacy-dataset",
        ])
        .output()
        .unwrap();
    assert_eq!(removed.status.code(), Some(2));
    assert!(removed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&removed.stderr).contains("unexpected argument '--dataset'"));
}

#[test]
fn run_publishes_one_manifest_and_only_public_output_facts() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let pack = pack(temporary.path());
    let legacy_run =
        data_home(temporary.path()).join("runs/019f6e00-0000-7000-8000-000000000099/manifest.json");
    let legacy_datasource = data_home(temporary.path()).join("datasources/alpha/sentinel");
    fs::create_dir_all(legacy_run.parent().unwrap()).unwrap();
    fs::create_dir_all(legacy_datasource.parent().unwrap()).unwrap();
    fs::write(&legacy_run, b"legacy root must not be read").unwrap();
    fs::write(&legacy_datasource, b"legacy datasource").unwrap();
    let captured = temporary.path().join("request.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(&pack)
        .arg("--")
        .args(["--limit", "5"])
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{"limit":"5","session":"__SESSION_ID__"},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
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
    assert_eq!(response["result"]["session_id"], session_id);
    let session_id = session_id.as_str();
    let run_id = response["result"]["run_id"].as_str().unwrap();
    assert_ne!(session_id, run_id);
    for identity in [session_id, run_id] {
        let identity = uuid::Uuid::parse_str(identity).unwrap();
        assert_eq!(identity.get_version_num(), 7);
    }
    assert_eq!(
        response["result"]["outputs"],
        serde_json::json!({"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}})
    );
    assert!(response["result"].get("inputs").is_none());
    assert!(response["result"].get("dataset").is_none());
    assert!(response.to_string().find("output_id").is_none());

    let manifest_path = data_home(temporary.path())
        .join("sessions")
        .join(session_id)
        .join("runs")
        .join(run_id)
        .join("manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["session_id"], session_id);
    assert_eq!(manifest["run_id"], run_id);
    assert_eq!(manifest["pack"], "alpha");
    assert_eq!(manifest["workflow"], "analyze");
    assert_eq!(manifest["child_runs"], serde_json::json!([]));
    assert!(manifest.get("dataset").is_none());
    assert_eq!(
        manifest["inputs"],
        serde_json::json!({"limit":"5", "session": session_id})
    );
    assert!(manifest["outputs"]["main"].get("output_id").is_none());
    assert_eq!(
        manifest["outputs"]["main"],
        serde_json::json!({"columns":[{"name":"value","type":"int64"}],"row_count":0})
    );

    let request: serde_json::Value = serde_json::from_slice(&fs::read(captured).unwrap()).unwrap();
    let session = dunce::canonicalize(
        data_home(temporary.path())
            .join("sessions")
            .join(session_id),
    )
    .unwrap();
    assert_eq!(request["candidate_id"], run_id);
    assert_eq!(request["workflow_name"], "analyze");
    assert_eq!(request["arguments"], serde_json::json!(["--limit", "5"]));
    assert!(request.get("inputs").is_none());
    assert!(request.get("dataset").is_none());
    assert_eq!(
        PathBuf::from(request["datasource_root"].as_str().unwrap()),
        session.join("materializations")
    );
    assert_eq!(
        PathBuf::from(request["candidate_path"].as_str().unwrap()),
        session.join("runs").join(run_id)
    );
    assert_eq!(
        PathBuf::from(request["scratch_root"].as_str().unwrap()),
        session.join("scratch").join(run_id)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(session.join("session.json")).unwrap()
        )
        .unwrap(),
        serde_json::json!({"session_id": session_id})
    );
    assert!(session.join("materializations").is_dir());
    assert_eq!(
        fs::read_dir(session.join("materializations"))
            .unwrap()
            .count(),
        0
    );
    assert!(session.join("runs").is_dir());
    assert!(session.join("scratch").is_dir());
    assert_eq!(fs::read_dir(session.join("scratch")).unwrap().count(), 0);
    let log_path = PathBuf::from(response["log_path"].as_str().unwrap());
    assert_eq!(
        log_path.file_name().unwrap().to_str().unwrap(),
        format!("run-{run_id}.log")
    );
    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains("fake Workflow output"));
    assert!(log.contains("scope: CLI preparation and Runtime execution"));
    assert!(log.contains("publication: manifest.json is the only published Run fact"));
    assert!(log.contains("runtime_status: success"));
    assert!(log.contains("publication_gate: ready"));
    assert!(!log.contains("dataset"));
    assert_eq!(
        fs::read(legacy_run).unwrap(),
        b"legacy root must not be read"
    );
    assert_eq!(fs::read(legacy_datasource).unwrap(), b"legacy datasource");
}

#[test]
fn sequential_nested_runs_publish_each_child_before_the_parent_records_them() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let parent_pack = pack(temporary.path());
    let child_pack = pack_named(temporary.path(), "child-pack", "beta");
    let captured = temporary.path().join("request.json");
    let nested_response = temporary.path().join("nested-response.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(&parent_pack)
        .arg("--pack-dir")
        .arg(&child_pack)
        .env("KAT_CAPTURE_REQUEST", captured)
        .env("KAT_FAKE_NESTED_PARENT_WORKFLOW", "analyze")
        .env("KAT_FAKE_NESTED_PACK", "beta")
        .env("KAT_FAKE_NESTED_WORKFLOW", "child")
        .env("KAT_FAKE_NESTED_CALL_COUNT", "2")
        .env("KAT_CAPTURE_NESTED_RESPONSE", &nested_response)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let parent_run_id = response["result"]["run_id"].as_str().unwrap();
    let runs = data_home(temporary.path())
        .join("sessions")
        .join(&session_id)
        .join("runs");
    let mut run_ids = fs::read_dir(&runs)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    run_ids.sort();
    assert_eq!(run_ids.len(), 3);
    let child_run_ids = run_ids
        .iter()
        .filter(|run_id| run_id.as_str() != parent_run_id)
        .collect::<Vec<_>>();
    assert_eq!(child_run_ids.len(), 2);
    let parent_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(runs.join(parent_run_id).join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        parent_manifest["child_runs"],
        serde_json::json!(child_run_ids)
    );
    for child_run_id in &child_run_ids {
        let child_manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(runs.join(child_run_id).join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(child_manifest["pack"], "beta");
        assert_eq!(child_manifest["workflow"], "child");
        assert_eq!(child_manifest["child_runs"], serde_json::json!([]));
    }

    let delivered: serde_json::Value =
        serde_json::from_slice(&fs::read(nested_response).unwrap()).unwrap();
    assert_eq!(delivered["status"], "success");
    assert_eq!(delivered["relations"][0]["name"], "main");
    let delivered_path = PathBuf::from(delivered["relations"][0]["path"].as_str().unwrap());
    assert!(child_run_ids.iter().any(|child_run_id| {
        delivered_path
            == dunce::canonicalize(runs.join(child_run_id).join("outputs/main.parquet")).unwrap()
    }));
}

#[test]
fn nested_manifests_record_only_direct_children_and_do_not_promote_orphans() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let parent_pack = pack(temporary.path());
    let child_pack = pack_named(temporary.path(), "child-pack", "beta");
    let grandchild_pack = pack_named(temporary.path(), "grandchild-pack", "gamma");

    for child_fails in [false, true] {
        let session_id = create_session(&binary, temporary.path());
        let captured = temporary
            .path()
            .join(format!("multilevel-{child_fails}-request.json"));
        let mut command = Command::new(&binary);
        command
            .arg("run")
            .args([
                "--session",
                &session_id,
                "--pack",
                "alpha",
                "--workflow",
                "analyze",
            ])
            .arg("--pack-dir")
            .arg(&parent_pack)
            .arg("--pack-dir")
            .arg(&child_pack)
            .arg("--pack-dir")
            .arg(&grandchild_pack)
            .env("KAT_CAPTURE_REQUEST", captured)
            .env("KAT_FAKE_NESTED_PARENT_WORKFLOW", "analyze")
            .env("KAT_FAKE_NESTED_PACK", "beta")
            .env("KAT_FAKE_NESTED_WORKFLOW", "child")
            .env("KAT_FAKE_SECOND_NESTED_PARENT_WORKFLOW", "child")
            .env("KAT_FAKE_SECOND_NESTED_PACK", "gamma")
            .env("KAT_FAKE_SECOND_NESTED_WORKFLOW", "grandchild")
            .env(
                "KAT_FAKE_RUNTIME_RESPONSE",
                r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
            );
        if child_fails {
            command
                .env("KAT_FAKE_RUNTIME_FAILURE_WORKFLOW", "child")
                .env("KAT_FAKE_CATCH_NESTED_FAILURE_WORKFLOW", "analyze");
        }
        configure(&mut command, temporary.path());

        let output = command.output().unwrap();

        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let manifests = manifests_by_workflow(temporary.path(), &session_id);
        let (parent_id, parent) = manifests.get("analyze").unwrap();
        let (grandchild_id, grandchild) = manifests.get("grandchild").unwrap();
        assert_eq!(grandchild["child_runs"], serde_json::json!([]));
        if child_fails {
            assert_eq!(manifests.len(), 2);
            assert!(!manifests.contains_key("child"));
            assert_eq!(parent["child_runs"], serde_json::json!([]));
        } else {
            assert_eq!(manifests.len(), 3);
            let (child_id, child) = manifests.get("child").unwrap();
            assert_eq!(parent["child_runs"], serde_json::json!([child_id]));
            assert_eq!(child["child_runs"], serde_json::json!([grandchild_id]));
        }
        assert_ne!(parent_id, grandchild_id);
    }
}

#[test]
fn nested_target_preflight_rejects_missing_and_ambiguous_workflows_before_allocation() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let parent_pack = pack(temporary.path());
    let child_pack = pack_named(temporary.path(), "child-pack", "beta");

    for (case, private_diagnostic) in [
        ("missing", "Workflow was not found"),
        ("ambiguous", "duplicate Workflow name"),
    ] {
        let session_id = create_session(&binary, temporary.path());
        let captured = temporary.path().join(format!("{case}-request.json"));
        let nested_response = temporary
            .path()
            .join(format!("{case}-nested-response.json"));
        let operations = temporary.path().join(format!("{case}-host-operations.txt"));
        let allocations = temporary.path().join(format!("{case}-allocations.txt"));
        let mut command = Command::new(&binary);
        command
            .arg("run")
            .args([
                "--session",
                &session_id,
                "--pack",
                "alpha",
                "--workflow",
                "analyze",
            ])
            .arg("--pack-dir")
            .arg(&parent_pack)
            .arg("--pack-dir")
            .arg(&child_pack)
            .env("KAT_CAPTURE_REQUEST", captured)
            .env("KAT_FAKE_NESTED_PARENT_WORKFLOW", "analyze")
            .env("KAT_FAKE_NESTED_PACK", "beta")
            .env("KAT_FAKE_NESTED_WORKFLOW", "child")
            .env("KAT_CAPTURE_NESTED_RESPONSE", &nested_response)
            .env("KAT_CAPTURE_HOST_OPERATIONS", &operations)
            .env("KAT_CAPTURE_PREFLIGHT_ALLOCATIONS", &allocations)
            .env("KAT_FAKE_NESTED_PREFLIGHT_FAILURE", private_diagnostic)
            .env(
                "KAT_FAKE_RUNTIME_RESPONSE",
                r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
            );
        configure(&mut command, temporary.path());

        let output = command.output().unwrap();

        assert_eq!(output.status.code(), Some(1));
        let public_response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(public_response["status"], "failure");
        assert!(!public_response.to_string().contains(private_diagnostic));
        let delivered: serde_json::Value =
            serde_json::from_slice(&fs::read(&nested_response).unwrap()).unwrap();
        assert_eq!(delivered["status"], "failure");
        assert_eq!(delivered["message"], "nested Workflow execution failed");
        assert_eq!(
            fs::read_to_string(&operations).unwrap(),
            "run_workflow:analyze\ninspect_workflow:child\n",
            "{case} preflight must not start the child execution Host"
        );
        assert_eq!(
            fs::read_to_string(&allocations).unwrap(),
            "1,1",
            "{case} preflight must run before child allocation"
        );
        let session = data_home(temporary.path())
            .join("sessions")
            .join(session_id);
        assert_eq!(fs::read_dir(session.join("runs")).unwrap().count(), 0);
        assert_eq!(fs::read_dir(session.join("scratch")).unwrap().count(), 0);
    }
}

#[test]
fn published_child_survives_when_the_parent_runtime_fails() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let parent_pack = pack(temporary.path());
    let child_pack = pack_named(temporary.path(), "child-pack", "beta");
    let captured = temporary.path().join("request.json");
    let nested_response = temporary.path().join("nested-response.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(&parent_pack)
        .arg("--pack-dir")
        .arg(&child_pack)
        .env("KAT_CAPTURE_REQUEST", captured)
        .env("KAT_FAKE_NESTED_PARENT_WORKFLOW", "analyze")
        .env("KAT_FAKE_NESTED_PACK", "beta")
        .env("KAT_FAKE_NESTED_WORKFLOW", "child")
        .env("KAT_CAPTURE_NESTED_RESPONSE", nested_response)
        .env("KAT_FAKE_PARENT_RUNTIME_FAILURE", "1")
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    let runs = data_home(temporary.path())
        .join("sessions")
        .join(&session_id)
        .join("runs");
    let entries = fs::read_dir(&runs)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    let child_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(entries[0].join("manifest.json")).unwrap()).unwrap();
    assert_eq!(child_manifest["pack"], "beta");
    assert_eq!(child_manifest["workflow"], "child");
    assert_eq!(child_manifest["child_runs"], serde_json::json!([]));
    assert_eq!(
        fs::read_dir(
            data_home(temporary.path())
                .join("sessions")
                .join(session_id)
                .join("scratch")
        )
        .unwrap()
        .count(),
        0
    );
}

#[test]
fn run_session_continues_the_same_materialization_scope_with_a_new_run() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let pack = pack(temporary.path());
    let beta_pack = pack_named(temporary.path(), "beta-pack", "beta");
    let first_capture = temporary.path().join("first-request.json");
    let mut first = Command::new(&binary);
    first
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(&pack)
        .env("KAT_CAPTURE_REQUEST", &first_capture)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut first, temporary.path());
    let first = first.output().unwrap();
    assert_eq!(first.status.code(), Some(0));
    let first_response: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first_response["result"]["session_id"], session_id);
    let session_id = session_id.as_str();
    let first_run_id = first_response["result"]["run_id"].as_str().unwrap();
    let session = data_home(temporary.path())
        .join("sessions")
        .join(session_id);
    fs::write(
        session.join("materializations").join("shared-evidence"),
        b"published",
    )
    .unwrap();

    let second_capture = temporary.path().join("second-request.json");
    let mut second = Command::new(&binary);
    second
        .arg("run")
        .args([
            "--session",
            session_id,
            "--pack",
            "beta",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(beta_pack)
        .env("KAT_CAPTURE_REQUEST", &second_capture)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut second, temporary.path());
    let second = second.output().unwrap();

    assert_eq!(
        second.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_response: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second_response["result"]["session_id"], session_id);
    let second_run_id = second_response["result"]["run_id"].as_str().unwrap();
    assert_ne!(second_run_id, first_run_id);
    let first_request: serde_json::Value =
        serde_json::from_slice(&fs::read(first_capture).unwrap()).unwrap();
    let second_request: serde_json::Value =
        serde_json::from_slice(&fs::read(second_capture).unwrap()).unwrap();
    assert_eq!(
        second_request["datasource_root"],
        first_request["datasource_root"]
    );
    assert_ne!(
        second_request["candidate_path"],
        first_request["candidate_path"]
    );
    assert_ne!(
        second_request["scratch_root"],
        first_request["scratch_root"]
    );
    assert_eq!(
        fs::read(session.join("materializations").join("shared-evidence")).unwrap(),
        b"published"
    );
    assert!(
        session
            .join("runs")
            .join(first_run_id)
            .join("manifest.json")
            .is_file()
    );
    let second_manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(
            session
                .join("runs")
                .join(second_run_id)
                .join("manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(second_manifest["pack"], "beta");
    assert_eq!(fs::read_dir(session.join("scratch")).unwrap().count(), 0);
}

#[test]
fn explicit_missing_session_fails_without_starting_runtime_or_creating_it() {
    const MISSING_SESSION: &str = "019f6e00-0000-7000-8000-000000000099";

    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let pack = pack(temporary.path());
    let captured = temporary.path().join("unexpected-request.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
            "--session",
            MISSING_SESSION,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        format!("Analysis Session {MISSING_SESSION} does not exist")
    );
    assert!(response.get("result").is_none());
    assert!(!captured.exists());
    assert!(!data_home(temporary.path()).join("sessions").exists());
}

#[test]
fn existing_session_failure_removes_replaced_candidate_and_scratch_entries() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let pack = pack(temporary.path());

    let initial_capture = temporary.path().join("initial-request.json");
    let mut initial = Command::new(&binary);
    initial
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(&pack)
        .env("KAT_CAPTURE_REQUEST", &initial_capture)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut initial, temporary.path());
    let initial = initial.output().unwrap();
    assert_eq!(initial.status.code(), Some(0));
    let initial_response: serde_json::Value = serde_json::from_slice(&initial.stdout).unwrap();
    assert_eq!(initial_response["result"]["session_id"], session_id);
    let session_id = session_id.as_str();
    let first_run_id = initial_response["result"]["run_id"].as_str().unwrap();
    let session = data_home(temporary.path())
        .join("sessions")
        .join(session_id);
    fs::write(
        session.join("materializations").join("retained"),
        b"published",
    )
    .unwrap();

    for (case, replacement, skip_outputs) in [
        ("scratch-file", "KAT_FAKE_REPLACE_SCRATCH_WITH_FILE", false),
        (
            "candidate-file",
            "KAT_FAKE_REPLACE_CANDIDATE_WITH_FILE",
            true,
        ),
    ] {
        let capture = temporary.path().join(format!("{case}-request.json"));
        let mut failed = Command::new(&binary);
        failed
            .arg("run")
            .args([
                "--session",
                session_id,
                "--pack",
                "alpha",
                "--workflow",
                "analyze",
            ])
            .arg("--pack-dir")
            .arg(&pack)
            .env("KAT_CAPTURE_REQUEST", &capture)
            .env(replacement, "1")
            .env(
                "KAT_FAKE_RUNTIME_RESPONSE",
                r#"{"status":"failure","error":{"message":"expected failure"}}"#,
            );
        if skip_outputs {
            failed.env("KAT_FAKE_SKIP_OUTPUTS", "1");
        }
        configure(&mut failed, temporary.path());

        let failed = failed.output().unwrap();

        assert_eq!(failed.status.code(), Some(1), "{case}");
        let request: serde_json::Value =
            serde_json::from_slice(&fs::read(capture).unwrap()).unwrap();
        assert!(
            !Path::new(request["candidate_path"].as_str().unwrap()).exists(),
            "{case}"
        );
        assert!(
            !Path::new(request["scratch_root"].as_str().unwrap()).exists(),
            "{case}"
        );
        assert!(session.join("session.json").is_file(), "{case}");
        assert!(
            session
                .join("runs")
                .join(first_run_id)
                .join("manifest.json")
                .is_file(),
            "{case}"
        );
        assert_eq!(
            fs::read(session.join("materializations").join("retained")).unwrap(),
            b"published",
            "{case}"
        );
    }
}

#[test]
fn active_run_blocks_session_delete_until_its_response_is_published() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let pack = pack(temporary.path());

    let first_capture = temporary.path().join("first-request.json");
    let mut first = Command::new(&binary);
    first
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(&pack)
        .env("KAT_CAPTURE_REQUEST", &first_capture)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut first, temporary.path());
    let first = first.output().unwrap();
    assert_eq!(first.status.code(), Some(0));
    let first_response: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first_response["result"]["session_id"], session_id);
    let session = data_home(temporary.path())
        .join("sessions")
        .join(&session_id);

    let capture = temporary.path().join("blocked-request.json");
    let response_written = temporary.path().join("response-written");
    let runtime_release = temporary.path().join("runtime-release");
    let mut active = Command::new(&binary);
    active
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(&pack)
        .env("KAT_CAPTURE_REQUEST", &capture)
        .env("KAT_FAKE_RESPONSE_WRITTEN", &response_written)
        .env("KAT_FAKE_RUNTIME_RELEASE", &runtime_release)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure(&mut active, temporary.path());
    let active = active.spawn().unwrap();
    wait_until_exists(&response_written);

    let mut deleting = Command::new(&binary);
    deleting.args(["session", "delete", "--session", &session_id]);
    configure(&mut deleting, temporary.path());
    let blocked_delete = deleting.output().unwrap();
    fs::write(&runtime_release, "release").unwrap();
    let active = active.wait_with_output().unwrap();

    assert_eq!(blocked_delete.status.code(), Some(1));
    let blocked_response: serde_json::Value =
        serde_json::from_slice(&blocked_delete.stdout).unwrap();
    assert_eq!(
        blocked_response["error"]["message"],
        format!("Analysis Session {session_id} is in use")
    );
    assert!(session.is_dir());
    assert!(
        !data_home(temporary.path())
            .join("sessions/.deletions")
            .join(&session_id)
            .exists()
    );
    assert_eq!(
        active.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&active.stderr)
    );

    let mut deleting = Command::new(&binary);
    deleting.args(["session", "delete", "--session", &session_id]);
    configure(&mut deleting, temporary.path());
    let deleted = deleting.output().unwrap();
    assert_eq!(
        deleted.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&deleted.stderr)
    );
    assert!(!session.exists());
    assert!(
        data_home(temporary.path())
            .join("sessions/.leases")
            .join(format!("{session_id}.lock"))
            .is_file()
    );
}

#[test]
fn two_runs_in_one_session_can_execute_concurrently() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let pack = pack(temporary.path());

    let initial_capture = temporary.path().join("initial-request.json");
    let mut initial = Command::new(&binary);
    initial
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(&pack)
        .env("KAT_CAPTURE_REQUEST", &initial_capture)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut initial, temporary.path());
    let initial = initial.output().unwrap();
    assert_eq!(initial.status.code(), Some(0));
    let initial_response: serde_json::Value = serde_json::from_slice(&initial.stdout).unwrap();
    assert_eq!(initial_response["result"]["session_id"], session_id);
    let initial_run_id = initial_response["result"]["run_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let release = temporary.path().join("concurrent-release");
    let mut markers = Vec::new();
    let mut children = Vec::new();
    for index in 0..2 {
        let capture = temporary
            .path()
            .join(format!("concurrent-{index}-request.json"));
        let marker = temporary
            .path()
            .join(format!("concurrent-{index}-response"));
        let mut run = Command::new(&binary);
        run.arg("run")
            .args([
                "--session",
                &session_id,
                "--pack",
                "alpha",
                "--workflow",
                "analyze",
            ])
            .arg("--pack-dir")
            .arg(&pack)
            .env("KAT_CAPTURE_REQUEST", capture)
            .env("KAT_FAKE_RESPONSE_WRITTEN", &marker)
            .env("KAT_FAKE_RUNTIME_RELEASE", &release)
            .env(
                "KAT_FAKE_RUNTIME_RESPONSE",
                r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure(&mut run, temporary.path());
        markers.push(marker);
        children.push(run.spawn().unwrap());
    }
    for marker in &markers {
        wait_until_exists(marker);
    }
    fs::write(&release, "release").unwrap();
    let outputs = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect::<Vec<_>>();

    let mut run_ids = Vec::new();
    for output in outputs {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["result"]["session_id"], session_id);
        run_ids.push(response["result"]["run_id"].as_str().unwrap().to_owned());
    }
    assert_ne!(run_ids[0], run_ids[1]);
    assert!(run_ids.iter().all(|run_id| run_id != &initial_run_id));
    let session = data_home(temporary.path())
        .join("sessions")
        .join(session_id);
    for run_id in std::iter::once(&initial_run_id).chain(run_ids.iter()) {
        assert!(
            session
                .join("runs")
                .join(run_id)
                .join("manifest.json")
                .is_file()
        );
    }
    assert_eq!(fs::read_dir(session.join("scratch")).unwrap().count(), 0);
}

#[test]
fn runtime_declared_outputs_must_exist_before_manifest_publication() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let pack = pack(temporary.path());
    let captured = temporary.path().join("request.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env("KAT_FAKE_SKIP_OUTPUTS", "1")
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    let request: serde_json::Value = serde_json::from_slice(&fs::read(&captured).unwrap()).unwrap();
    let candidate_id = request["candidate_id"].as_str().unwrap();
    let candidate_path = PathBuf::from(request["candidate_path"].as_str().unwrap());
    assert!(!response["error"].to_string().contains(candidate_id));
    assert!(!candidate_path.exists());
    let session = data_home(temporary.path())
        .join("sessions")
        .join(session_id);
    assert_eq!(fs::read_dir(session.join("runs")).unwrap().count(), 0);
    assert_eq!(fs::read_dir(session.join("scratch")).unwrap().count(), 0);
}

#[test]
fn runtime_declared_outputs_must_be_valid_parquet_before_manifest_publication() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let pack = pack(temporary.path());
    let captured = temporary.path().join("request.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env("KAT_FAKE_CORRUPT_OUTPUT", "1")
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    let request: serde_json::Value = serde_json::from_slice(&fs::read(&captured).unwrap()).unwrap();
    assert!(!Path::new(request["candidate_path"].as_str().unwrap()).exists());
    let session = data_home(temporary.path())
        .join("sessions")
        .join(session_id);
    assert_eq!(fs::read_dir(session.join("runs")).unwrap().count(), 0);
    assert_eq!(fs::read_dir(session.join("scratch")).unwrap().count(), 0);
}

#[test]
fn run_waits_for_the_direct_runtime_to_exit_before_publishing() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let pack = pack(temporary.path());
    let captured = temporary.path().join("request.json");
    let response_written = temporary.path().join("response-written");
    let runtime_release = temporary.path().join("runtime-release");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env("KAT_FAKE_RESPONSE_WRITTEN", &response_written)
        .env("KAT_FAKE_RUNTIME_RELEASE", &runtime_release)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure(&mut command, temporary.path());

    let child = command.spawn().unwrap();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || sender.send(child.wait_with_output()).unwrap());
    wait_until_exists(&response_written);
    wait_until_exists(&captured);
    let request: serde_json::Value = serde_json::from_slice(&fs::read(&captured).unwrap()).unwrap();
    let manifest = Path::new(request["candidate_path"].as_str().unwrap()).join("manifest.json");
    let early_output = match receiver.recv_timeout(Duration::from_secs(2)) {
        Ok(output) => Some(output.unwrap()),
        Err(mpsc::RecvTimeoutError::Timeout) => None,
        Err(error) => panic!("kat wait channel failed: {error}"),
    };
    let manifest_was_published_early = manifest.exists();
    fs::write(&runtime_release, "release").unwrap();
    let exited_early = early_output.is_some();
    let output = match early_output {
        Some(output) => output,
        None => receiver.recv().unwrap().unwrap(),
    };

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !exited_early,
        "kat run published success before its direct Runtime exited"
    );
    assert!(
        !manifest_was_published_early,
        "kat run published manifest.json before its direct Runtime exited"
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert!(log.contains("fake Workflow output"));
}

#[test]
#[ignore = "requires KAT_TEST_PYTHON and a wheel built from the current checkout"]
fn run_composes_real_installed_workflows_end_to_end() {
    let python = PathBuf::from(
        std::env::var_os("KAT_TEST_PYTHON").expect("KAT_TEST_PYTHON identifies CPython"),
    );
    let workflow_wheel = PathBuf::from(
        std::env::var_os("KAT_TEST_WORKFLOW_WHEEL")
            .expect("KAT_TEST_WORKFLOW_WHEEL identifies the current wheel"),
    );
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) =
        support::stage_real_host_skill(temporary.path(), &cargo_kat(), &python, &workflow_wheel);
    let session_id = create_session(&binary, temporary.path());
    let parent_pack = pack(temporary.path());
    let parent_workflows = parent_pack.join("workflows");
    fs::create_dir(&parent_workflows).unwrap();
    fs::write(
        parent_workflows.join("compose.py"),
        r#"from kat import Context, dataprovider as dp, workflow

@workflow(
    name="compose",
    description="Compose a child Workflow's published facts.",
)
def compose(ctx: Context):
    child = ctx.run("beta", "facts", base=20)
    return dp.DataFusionProvider(catalog=child).query(
        "SELECT SUM(value) AS total FROM facts"
    )
"#,
    )
    .unwrap();
    let child_pack = pack_named(temporary.path(), "child-pack", "beta");
    let child_workflows = child_pack.join("workflows");
    fs::create_dir(&child_workflows).unwrap();
    fs::write(
        child_workflows.join("facts.py"),
        r#"import pyarrow as pa
from kat import Context, dataprovider as dp, workflow

@workflow(
    name="facts",
    description="Publish facts for a parent Workflow.",
    parameters={"base": "First value to publish."},
)
def facts(ctx: Context, base: int):
    del ctx
    return {"facts": dp.Table.from_arrow(pa.table({"value": [base, base + 2]}))}
"#,
    )
    .unwrap();
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "compose",
        ])
        .arg("--pack-dir")
        .arg(&parent_pack)
        .arg("--pack-dir")
        .arg(&child_pack);
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
    assert_eq!(response["result"]["outputs"]["main"]["row_count"], 1);
    assert_eq!(
        response["result"]["outputs"]["main"]["columns"],
        serde_json::json!([{"name":"total","type":"int64"}])
    );
    let session_id = response["result"]["session_id"].as_str().unwrap();
    let parent_run_id = response["result"]["run_id"].as_str().unwrap();

    let mut inspect = Command::new(&binary);
    inspect.args(["inspect", "session", "--session", session_id]);
    configure(&mut inspect, temporary.path());
    let inspected = inspect.output().unwrap();
    assert_eq!(
        inspected.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let inventory: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inventory["status"], "success");
    let runs = inventory["result"]["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    let parent = runs
        .iter()
        .find(|run| run["run_id"] == parent_run_id)
        .unwrap();
    let child = runs.iter().find(|run| run["workflow"] == "facts").unwrap();
    let child_run_id = child["run_id"].as_str().unwrap();
    assert_eq!(parent["pack"], "alpha");
    assert_eq!(parent["workflow"], "compose");
    assert_eq!(parent["child_runs"], serde_json::json!([child_run_id]));
    assert_eq!(child["pack"], "beta");
    assert_eq!(child["child_runs"], serde_json::json!([]));
    assert_eq!(child["outputs"]["facts"]["row_count"], 2);

    let run_root = data_home(temporary.path())
        .join("sessions")
        .join(session_id)
        .join("runs");
    let parent_run = run_root.join(parent_run_id);
    let child_run = run_root.join(child_run_id);
    let parent_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(parent_run.join("manifest.json")).unwrap()).unwrap();
    let child_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(child_run.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(
        parent_manifest["child_runs"],
        serde_json::json!([child_run_id])
    );
    assert_eq!(child_manifest["child_runs"], serde_json::json!([]));
    assert!(parent_run.join("outputs/main.parquet").is_file());
    assert!(child_run.join("outputs/facts.parquet").is_file());

    assert_eq!(
        query_rows(
            &binary,
            temporary.path(),
            session_id,
            parent_run_id,
            "SELECT total FROM output.main",
        ),
        vec![serde_json::json!({"total": 42})]
    );
    assert_eq!(
        query_rows(
            &binary,
            temporary.path(),
            session_id,
            child_run_id,
            "SELECT value FROM output.facts ORDER BY value",
        ),
        vec![
            serde_json::json!({"value": 20}),
            serde_json::json!({"value": 22}),
        ]
    );
    assert!(!parent_workflows.join("__pycache__").exists());
    assert!(!child_workflows.join("__pycache__").exists());
}

#[test]
fn run_log_projects_user_controlled_text() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let pack = pack(temporary.path());
    let captured = temporary.path().join("projected-request.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze\nforged: true\u{001b}[31m",
        ])
        .arg("--pack-dir")
        .arg(pack)
        .arg("--")
        .args(["--value", "line one\nline two\u{0007}"])
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert!(log.contains("workflow: analyze\\nforged: true"));
    assert!(log.contains("line one\\nline two"));
    assert!(!log.contains('\u{001b}'));
    assert!(!log.contains('\u{0007}'));
    assert!(!log.lines().any(|line| line == "forged: true"));
}

#[test]
fn runtime_failure_never_publishes_or_exposes_the_candidate() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let pack = pack(temporary.path());
    let captured = temporary.path().join("failed-request.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
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
    let candidate_path = PathBuf::from(request["candidate_path"].as_str().unwrap());
    assert!(!response["error"].to_string().contains(candidate_id));
    assert!(!candidate_path.exists());
    assert!(candidate_path.parent().unwrap().parent().unwrap().exists());
}

#[test]
fn untrusted_runtime_response_never_exposes_the_candidate() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let pack = pack(temporary.path());
    for (case, runtime_response) in [
        (
            "effective-input-name",
            r#"{"status":"success","result":{"effective_inputs":{"__CANDIDATE_ID__":"value"},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        ),
        (
            "effective-input-value",
            r#"{"status":"success","result":{"effective_inputs":{"value":"__CANDIDATE_ID__"},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        ),
        (
            "output-name",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"__CANDIDATE_ID__":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        ),
        (
            "column-name",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"__CANDIDATE_ID__","type":"int64"}],"row_count":0}}}}"#,
        ),
        (
            "column-type",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"__CANDIDATE_ID__"}],"row_count":0}}}}"#,
        ),
        (
            "response-status",
            r#"{"status":"__CANDIDATE_ID__","result":{}}"#,
        ),
    ] {
        let case_root = temporary.path().join(case);
        fs::create_dir(&case_root).unwrap();
        let session_id = create_session(&binary, &case_root);
        let captured = case_root.join("request.json");
        let mut command = Command::new(&binary);
        command
            .arg("run")
            .args([
                "--session",
                &session_id,
                "--pack",
                "alpha",
                "--workflow",
                "analyze",
            ])
            .arg("--pack-dir")
            .arg(&pack)
            .env("KAT_CAPTURE_REQUEST", &captured)
            .env("KAT_FAKE_RUNTIME_RESPONSE", runtime_response);
        configure(&mut command, &case_root);

        let output = command.output().unwrap();

        assert_eq!(output.status.code(), Some(1), "{case}");
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let request: serde_json::Value =
            serde_json::from_slice(&fs::read(captured).unwrap()).unwrap();
        let candidate_id = request["candidate_id"].as_str().unwrap();
        let candidate_path = PathBuf::from(request["candidate_path"].as_str().unwrap());
        assert_eq!(response["status"], "failure", "{case}");
        assert!(response.get("result").is_none(), "{case}");
        assert_eq!(
            response["error"]["message"], "Workflow Runtime failed",
            "{case}"
        );
        assert_eq!(
            response["error"]["causes"],
            serde_json::json!(["Runtime Response is not valid for the requested operation"]),
            "{case}"
        );
        assert!(
            !response["error"].to_string().contains(candidate_id),
            "{case}"
        );
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains(candidate_id),
            "{case}"
        );
        assert!(!candidate_path.exists(), "{case}");
        assert!(
            candidate_path.parent().unwrap().parent().unwrap().exists(),
            "{case}"
        );
        let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
        assert!(log.contains("status: failure"), "{case}");
    }
}

#[test]
fn operation_log_creation_failure_never_starts_runtime_or_publishes() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let pack = pack(temporary.path());
    let captured = temporary.path().join("unexpected-request.json");
    fs::create_dir_all(data_home(temporary.path())).unwrap();
    fs::write(data_home(temporary.path()).join("logs"), "not a directory").unwrap();
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    assert!(!captured.exists());
    assert!(
        data_home(temporary.path())
            .join("sessions")
            .join(session_id)
            .is_dir()
    );
    assert!(response.get("log_path").is_none());
}

#[test]
fn session_allocation_failure_is_completed_through_its_run_log() {
    const SESSION_ID: &str = "019f6e00-0000-7000-8000-000000000098";

    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let pack = pack(temporary.path());
    let captured = temporary.path().join("unexpected-candidate-request.json");
    fs::create_dir_all(data_home(temporary.path())).unwrap();
    fs::write(
        data_home(temporary.path()).join("sessions"),
        "not a directory",
    )
    .unwrap();
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
            "--session",
            SESSION_ID,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
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
            .contains("Analysis Session storage layout is invalid")
    );
}

#[test]
fn manifest_publication_failure_never_returns_or_publishes_a_run() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let session_id = create_session(&binary, temporary.path());
    let pack = pack(temporary.path());
    let captured = temporary.path().join("manifest-fault-request.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
            "--session",
            &session_id,
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
        ])
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_CAPTURE_REQUEST", &captured)
        .env("KAT_FAKE_MANIFEST_DIRECTORY", "1")
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
        );
    configure(&mut command, temporary.path());

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    let request: serde_json::Value = serde_json::from_slice(&fs::read(captured).unwrap()).unwrap();
    let candidate_id = request["candidate_id"].as_str().unwrap();
    let candidate_path = PathBuf::from(request["candidate_path"].as_str().unwrap());
    assert!(!response["error"].to_string().contains(candidate_id));
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert!(log.contains("publication_gate: ready"));
    assert!(!candidate_path.exists());
    assert!(candidate_path.parent().unwrap().parent().unwrap().exists());
}
