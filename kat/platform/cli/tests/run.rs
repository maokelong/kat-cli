use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use base64::Engine;

mod support;
use support::cargo_kat;

const PARQUET: &str = "UEFSMRUEFSAVIEwVBBUAEgAAAQAAAAAAAAACAAAAAAAAABUAFRIVEiwVBBUQFQYVBgAAAgAAAAQBAQMCFQQVMBUwTBUEFQASAAAGAAAAY2FsbGVyCgAAAGZ1dGV4X3dhaXQVABUSFRIsFQQVEBUGFQYAAAIAAAAEAQEDAhkSAhkYCAEAAAAAAAAAGRgIAgAAAAAAAAAVAhkWACkmAAQAGRICGRgGY2FsbGVyGRgKZnV0ZXhfd2FpdBUCGRYAKSYABAAZHBZEFTQWAAAAGRwWxAEVNBYAABkWIAAVAhk8SAxhcnJvd19zY2hlbWEVBAAVBCUCGAJpZAAVDCUCGARkYXRhJQBMHAAAABYEGRwZLCYAHBUEGTUABhAZGAJpZBUAFgQWcBZwJkQmCBwYCAIAAAAAAAAAGAgBAAAAAAAAABYAKAgCAAAAAAAAABgIAQAAAAAAAAAREQAZLBUEFQAVAgAVABUQFQIAPDkmAAQAABaEAxUUFvgBFUYAJgAcFQwZNQAGEBkYBGRhdGEVABYEFoABFoABJsQBJngcNgAoCmZ1dGV4X3dhaXQYBmNhbGxlchERABksFQQVABUCABUAFRAVAgA8FiApJgAEAAAWmAMVHBa+AhVGABbwARYEJggW8AEUAAAZHBgMQVJST1c6c2NoZW1hGOwBLy8vLy82Z0FBQUFRQUFBQUFBQUtBQXdBQ2dBSkFBUUFDZ0FBQUJBQUFBQUFBUVFBQ0FBSUFBQUFCQUFJQUFBQUJBQUFBQUlBQUFCRUFBQUFCQUFBQU5ULy8vOFlBQUFBREFBQUFBQUFBUVVRQUFBQUFBQUFBQVFBQkFBRUFBQUFCQUFBQUdSaGRHRUFBQUFBRUFBVUFCQUFEZ0FQQUFRQUFBQUlBQkFBQUFBWUFBQUFJQUFBQUFBQUFRSWNBQUFBQ0FBTUFBUUFDd0FJQUFBQVFBQUFBQUFBQUFFQUFBQUFBZ0FBQUdsa0FBQT0AGBlwYXJxdWV0LXJzIHZlcnNpb24gNTguMy4wGSwcAAAcAAAALwIAAFBBUjE=";

fn stage_skill(root: &Path) -> (PathBuf, PathBuf) {
    support::stage_skill(root, "skill")
}

fn stage_fake_host(binary: &Path) {
    let payload = binary.parent().unwrap();
    let host = support::host_path(binary);
    fs::create_dir_all(host.parent().unwrap()).unwrap();
    let source = payload.join("fake-run-host.rs");
    fs::write(
        &source,
        r#"
use std::{
    env, fs,
    path::Path,
    process,
    thread,
    time::Duration,
};

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
        || !request.contains("\"candidate_path\":")
    {
        process::exit(92);
    }
    fs::write(env::var("KAT_CAPTURE_REQUEST").unwrap(), &request).unwrap();
    if env::var_os("KAT_FAKE_MANIFEST_DIRECTORY").is_some() {
        let candidate_path = json_string(&request, "candidate_path");
        fs::create_dir(Path::new(&candidate_path).join("manifest.json")).unwrap();
    }
    if env::var_os("KAT_FAKE_SKIP_OUTPUTS").is_none() {
        let candidate_path = json_string(&request, "candidate_path");
        let outputs = Path::new(&candidate_path).join("outputs");
        fs::create_dir(&outputs).unwrap();
        fs::write(outputs.join("main.parquet"), b"opaque output").unwrap();
    }
    println!("fake Workflow output");
    let mut response = env::var("KAT_FAKE_RUNTIME_RESPONSE").unwrap();
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
    command.env_remove("KAT_DATA_HOME");

    #[cfg(not(windows))]
    command
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("HOME", root.join("home"));
    #[cfg(windows)]
    let _ = (command, root);
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

#[cfg(not(windows))]
fn data_home(root: &Path) -> PathBuf {
    root.join("xdg-data/kat")
}

#[cfg(windows)]
fn data_home(_root: &Path) -> PathBuf {
    directories::ProjectDirs::from("", "", "KAT")
        .expect("Windows runner has a standard user data directory")
        .data_dir()
        .to_path_buf()
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
fn run_help_discloses_operation_log_inputs() {
    let output = Command::new(cargo_kat())
        .args(["run", "--help"])
        .output()
        .expect("run operation help");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(help.contains("Operation log may retain the resolved PACK"));
    assert!(help.contains("optional Dataset path"));
    assert!(help.contains("all arguments after"));
    assert!(help.contains("Do not pass secrets"));
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires an isolated Windows user profile; full-ci runs it on windows-latest"
)]
fn run_publishes_one_manifest_and_only_public_output_facts() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
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
            r#"{"status":"success","result":{"effective_inputs":{"limit":"5"},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
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
    assert!(manifest["outputs"]["main"].get("output_id").is_none());
    assert_eq!(
        manifest["outputs"]["main"],
        serde_json::json!({"columns":[{"name":"value","type":"int64"}],"row_count":0})
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
    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains("fake Workflow output"));
    assert!(log.contains("scope: CLI preparation and Runtime execution"));
    assert!(log.contains("publication: manifest.json is the only published Run fact"));
    assert!(log.contains("runtime_status: success"));
    assert!(log.contains("publication_gate: ready"));
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires an isolated Windows user profile; full-ci runs it on windows-latest"
)]
fn runtime_success_response_is_the_authority_for_output_materialization() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let pack = pack(temporary.path());
    let captured = temporary.path().join("request.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args(["--pack", "alpha", "--workflow", "analyze"])
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

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "success");
    let run_id = response["result"]["run_id"].as_str().unwrap();
    let run_path = data_home(temporary.path()).join("runs").join(run_id);
    assert!(run_path.join("manifest.json").is_file());
    assert!(!run_path.join("outputs").join("main.parquet").exists());
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires an isolated Windows user profile; full-ci runs it on windows-latest"
)]
fn run_waits_for_the_direct_runtime_to_exit_before_publishing() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let pack = pack(temporary.path());
    let captured = temporary.path().join("request.json");
    let response_written = temporary.path().join("response-written");
    let runtime_release = temporary.path().join("runtime-release");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args(["--pack", "alpha", "--workflow", "analyze"])
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
fn run_uses_real_installed_workflow_host_end_to_end() {
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
    let pack = pack(temporary.path());
    let workflows = pack.join("workflows");
    fs::create_dir(&workflows).unwrap();
    fs::write(
        workflows.join("analyze.py"),
        r#"from kat import Context, workflow

@workflow(
    name="analyze",
    title="Analyze",
    required_tables=["data_dict"],
)
def analyze(ctx: Context):
    """Analyze the Dataset."""
    return ctx.sql("select id, data from data_dict order by id")
"#,
    )
    .unwrap();
    let dataset = dataset(temporary.path());
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args(["--pack", "alpha", "--workflow", "analyze"])
        .arg("--pack-dir")
        .arg(pack)
        .arg("--dataset")
        .arg(&dataset);
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
    assert_eq!(response["result"]["outputs"]["main"]["row_count"], 2);
    assert_eq!(
        response["result"]["outputs"]["main"]["columns"],
        serde_json::json!([
            {"name":"id","type":"int64"},
            {"name":"data","type":"string_view"}
        ])
    );
    let run_id = response["result"]["run_id"].as_str().unwrap();
    let run = data_home(temporary.path()).join("runs").join(run_id);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(run.join("manifest.json")).unwrap()).unwrap();
    assert!(manifest["outputs"]["main"].get("output_id").is_none());
    assert!(run.join("outputs").join("main.parquet").is_file());
    assert!(!workflows.join("__pycache__").exists());
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires an isolated Windows user profile; full-ci runs it on windows-latest"
)]
fn run_log_projects_user_controlled_text() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
    let (_skill, binary) = stage_skill(temporary.path());
    stage_fake_host(&binary);
    let pack = pack(temporary.path());
    let captured = temporary.path().join("projected-request.json");
    let mut command = Command::new(&binary);
    command
        .arg("run")
        .args([
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
#[cfg_attr(
    windows,
    ignore = "requires an isolated Windows user profile; full-ci runs it on windows-latest"
)]
fn runtime_failure_never_publishes_or_exposes_the_candidate() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
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
            .exists()
    );
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires an isolated Windows user profile; full-ci runs it on windows-latest"
)]
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
        let captured = case_root.join("request.json");
        let mut command = Command::new(&binary);
        command
            .arg("run")
            .args(["--pack", "alpha", "--workflow", "analyze"])
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
        assert!(
            !data_home(&case_root)
                .join("runs")
                .join(candidate_id)
                .exists(),
            "{case}"
        );
        let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
        assert!(log.contains("status: failure"), "{case}");
    }
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires an isolated Windows user profile; full-ci runs it on windows-latest"
)]
fn operation_log_creation_failure_never_starts_runtime_or_publishes() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
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
            r#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#,
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
#[cfg_attr(
    windows,
    ignore = "requires an isolated Windows user profile; full-ci runs it on windows-latest"
)]
fn candidate_creation_failure_is_completed_through_its_run_log() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
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
            .contains("failed to create Run root")
    );
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires an isolated Windows user profile; full-ci runs it on windows-latest"
)]
fn manifest_publication_failure_never_returns_or_publishes_a_run() {
    let temporary = tempfile::tempdir().unwrap();
    let _data_home = PlatformDataHomeGuard::new(temporary.path());
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
    assert!(!response["error"].to_string().contains(candidate_id));
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert!(log.contains("publication_gate: ready"));
    assert!(
        !data_home(temporary.path())
            .join("runs")
            .join(candidate_id)
            .exists()
    );
}
