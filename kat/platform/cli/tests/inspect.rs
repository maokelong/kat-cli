use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

mod support;
use support::cargo_kat;

#[path = "support/test_home.rs"]
mod test_home;

const INSPECT_RUN_ID: &str = "019f6e00-0000-7000-8000-000000000235";

fn stage_minimum_skill_layout(root: &Path) -> (PathBuf, PathBuf) {
    support::stage_skill(root, "movable-skill")
}

fn stage_fake_python_host(binary: &Path) {
    let payload = binary.parent().expect("Platform Payload directory");
    let host = support::host_path(binary);
    fs::create_dir_all(host.parent().unwrap()).expect("create fake Host directory");
    let source = payload.join("fake-python-host.rs");
    fs::write(
        &source,
        r###"
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
    let operation = env::var("KAT_EXPECT_OPERATION").unwrap_or_else(|_| "inspect_workflow".to_owned());
    let name_field = env::var("KAT_EXPECT_NAME_FIELD").unwrap_or_else(|_| "workflow_name".to_owned());
    let expected_name = env::var("KAT_EXPECT_NAME").unwrap_or_default();
    let expected_selector = if expected_name.is_empty() {
        format!(r#""{}":null"#, name_field)
    } else {
        format!(r#""{}":"{}""#, name_field, expected_name)
    };
    if !request.contains(&format!(r#""operation":"{}""#, operation))
        || !request.contains("\"pack_name\":")
        || !request.contains("\"pack_path\":")
        || !request.contains(&expected_selector)
    {
        process::exit(92);
    }
    io::stdout().write_all(b"\x1b[31mruntime stdout\x1b[0m\r\ninvalid: \xff\r").unwrap();
    io::stderr().write_all(b"runtime stderr\r\n").unwrap();
    fs::write(&arguments[10], env::var("KAT_FAKE_RUNTIME_RESPONSE").unwrap()).unwrap();
    process::exit(env::var("KAT_FAKE_RUNTIME_EXIT").unwrap_or_else(|_| "0".to_owned()).parse().unwrap());
}

"###,
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

fn prepare_platform_data_home(command: &mut Command, root: &Path) {
    command.env_remove("KAT_DATA_HOME");

    #[cfg(not(windows))]
    command
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("HOME", root.join("home"));
    #[cfg(windows)]
    {
        // ProjectDirs 使用 Windows Known Folder API；进程测试因此使用干净 runner 的真实 Data Home。
        let _ = (command, root);
        assert_clean_windows_data_home();
    }
}

#[cfg(not(windows))]
fn data_home(root: &Path) -> PathBuf {
    root.join("xdg-data").join("kat")
}

#[cfg(windows)]
fn assert_clean_windows_data_home() {
    let project_dirs = directories::ProjectDirs::from("", "", "KAT")
        .expect("Windows runner has a standard user data directory");
    let pack_directory = project_dirs.data_dir().join("packs");
    assert!(
        !pack_directory.exists(),
        "Windows real-process tests require a clean runner without {pack_directory:?}"
    );
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
        .arg("workflow")
        .args(["--pack", "alpha"])
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_EXPECT_OPERATION", "inspect_workflow")
        .env("KAT_EXPECT_NAME_FIELD", "workflow_name")
        .env("KAT_EXPECT_NAME", "")
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"workflows":[]}}"#,
        );
    prepare_platform_data_home(&mut command, root);
    command
}

fn knowledge_inspect_command(
    binary: &Path,
    root: &Path,
    pack: &Path,
    kind: &str,
    selected: Option<&str>,
) -> Command {
    let mut command = Command::new(binary);
    command.arg("inspect").arg(kind).args(["--pack", "alpha"]);
    if let Some(selected) = selected {
        command.arg(format!("--{kind}")).arg(selected);
    }
    command
        .arg("--pack-dir")
        .arg(pack)
        .env("KAT_EXPECT_OPERATION", format!("inspect_{kind}"))
        .env("KAT_EXPECT_NAME_FIELD", format!("{kind}_name"))
        .env("KAT_EXPECT_NAME", selected.unwrap_or_default());
    prepare_platform_data_home(&mut command, root);
    command
}

#[test]
fn workflow_inspection_forwards_the_exact_request_and_public_result() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_minimum_skill_layout(temporary.path());
    stage_knowledge_inspection_host(&binary);
    let pack = temporary.path().join("external-checkout");
    write_pack(&pack, "alpha", "External PACK");

    let mut list = knowledge_inspect_command(&binary, temporary.path(), &pack, "workflow", None);
    list.env(
        "KAT_FAKE_RUNTIME_RESPONSE",
        r#"{"status":"success","result":{"workflows":[{"name":"cpu-time","description":"Analyze CPU time."}]}}"#,
    );
    let list = list.output().expect("inspect Workflow list");
    assert_eq!(
        list.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&list.stdout).unwrap()["result"],
        serde_json::json!({
            "workflows": [{"name": "cpu-time", "description": "Analyze CPU time."}]
        })
    );

    let mut detail = knowledge_inspect_command(
        &binary,
        temporary.path(),
        &pack,
        "workflow",
        Some("cpu-time"),
    );
    detail.env(
        "KAT_FAKE_RUNTIME_RESPONSE",
        r#"{"status":"success","result":{"workflow":{"name":"cpu-time","description":"Analyze CPU time.","parameters":[],"guide":null}}}"#,
    );
    let detail = detail.output().expect("inspect one Workflow");
    assert_eq!(
        detail.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&detail.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&detail.stdout).unwrap()["result"],
        serde_json::json!({
            "workflow": {
                "name": "cpu-time",
                "description": "Analyze CPU time.",
                "parameters": [],
                "guide": null
            }
        })
    );
}

#[test]
fn run_workflow_inspection_resolves_the_current_pack_and_workflow() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_minimum_skill_layout(temporary.path());
    stage_knowledge_inspection_host(&binary);
    let pack = temporary.path().join("external-checkout");
    write_pack(&pack, "alpha", "External PACK");
    let run = test_home::data_home(temporary.path())
        .join("runs")
        .join(INSPECT_RUN_ID);
    fs::create_dir_all(&run).expect("create published Run directory");
    fs::write(
        run.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "run_id": INSPECT_RUN_ID,
            "pack": "alpha",
            "workflow": "analyze",
            "dataset": {
                "historical": ["shape", "is", "irrelevant", "to", "inspect"]
            },
            "inputs": ["historical", "shape"],
            "outputs": "historical shape"
        }))
        .unwrap(),
    )
    .expect("write published Run manifest");
    let mut command = Command::new(binary);
    command
        .arg("inspect")
        .arg("workflow")
        .args(["--run", INSPECT_RUN_ID])
        .arg("--pack-dir")
        .arg(&pack)
        .env("KAT_EXPECT_OPERATION", "inspect_workflow")
        .env("KAT_EXPECT_NAME_FIELD", "workflow_name")
        .env("KAT_EXPECT_NAME", "analyze")
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r##"{"status":"success","result":{"workflow":{"name":"analyze","description":"Analyze current Run facts.","parameters":[],"guide":"# Current guide\n"}}}"##,
        );
    test_home::configure(&mut command, temporary.path());

    let output = command
        .output()
        .expect("inspect one Run's current Workflow");

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response["result"],
        serde_json::json!({
            "workflow": {
                "name": "analyze",
                "description": "Analyze current Run facts.",
                "parameters": [],
                "guide": "# Current guide\n"
            }
        })
    );
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert!(log.contains("operation: kat inspect workflow"));
    assert!(log.contains(&format!("run: {INSPECT_RUN_ID}")));
    assert!(log.contains("pack: alpha"));
    assert!(log.contains("workflow: analyze"));
}

fn stage_knowledge_inspection_host(binary: &Path) {
    let payload = binary.parent().expect("Platform Payload directory");
    let host = support::host_path(binary);
    fs::create_dir_all(host.parent().unwrap()).expect("create fake Host directory");
    let source = payload.join("fake-knowledge-inspection-host.rs");
    fs::write(
        &source,
        r###"
use std::{env, fs, process};

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
    let operation = env::var("KAT_EXPECT_OPERATION").unwrap();
    let name_field = env::var("KAT_EXPECT_NAME_FIELD").unwrap();
    let expected_name = env::var("KAT_EXPECT_NAME").unwrap();
    let expected_selector = if expected_name.is_empty() {
        format!(r#""{}":null"#, name_field)
    } else {
        format!(r#""{}":"{}""#, name_field, expected_name)
    };
    if !request.contains(&format!(r#""operation":"{}""#, operation))
        || !request.contains("\"pack_name\":\"alpha\"")
        || !request.contains("\"pack_path\":")
        || !request.contains(&expected_selector)
    {
        process::exit(92);
    }
    fs::write(&arguments[10], env::var("KAT_FAKE_RUNTIME_RESPONSE").unwrap()).unwrap();
}
"###,
    )
    .expect("write fake knowledge inspection Host source");
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = Command::new(rustc)
        .arg(&source)
        .arg("-o")
        .arg(&host)
        .output()
        .expect("compile fake knowledge inspection Host");
    assert!(
        output.status.success(),
        "fake knowledge inspection Host compilation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn provider_inspection_forwards_the_exact_request_and_public_result() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_minimum_skill_layout(temporary.path());
    stage_knowledge_inspection_host(&binary);
    let pack = temporary.path().join("external-checkout");
    write_pack(&pack, "alpha", "External PACK");

    let mut list = knowledge_inspect_command(&binary, temporary.path(), &pack, "provider", None);
    list.env(
        "KAT_FAKE_RUNTIME_RESPONSE",
        r#"{"status":"success","result":{"providers":[{"name":"postgresql","description":"Query PostgreSQL."}]}}"#,
    );
    let list = list.output().expect("inspect Provider list");
    assert_eq!(
        list.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&list.stdout).unwrap()["result"],
        serde_json::json!({
            "providers": [{"name": "postgresql", "description": "Query PostgreSQL."}]
        })
    );

    let mut detail = knowledge_inspect_command(
        &binary,
        temporary.path(),
        &pack,
        "provider",
        Some("postgresql"),
    );
    detail.env(
        "KAT_FAKE_RUNTIME_RESPONSE",
        r##"{"status":"success","result":{"provider":{"name":"postgresql","description":"Query PostgreSQL.","module":"kat.pack.datasources.postgresql","qualname":"PostgreSQLProvider","guide":"# PostgreSQL\n"}}}"##,
    );
    let detail = detail.output().expect("inspect one Provider");
    assert_eq!(
        detail.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&detail.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&detail.stdout).unwrap()["result"],
        serde_json::json!({
            "provider": {
                "name": "postgresql",
                "description": "Query PostgreSQL.",
                "module": "kat.pack.datasources.postgresql",
                "qualname": "PostgreSQLProvider",
                "guide": "# PostgreSQL\n"
            }
        })
    );
}

#[test]
fn workflow_detail_rejects_a_missing_guide_field_but_accepts_null() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_minimum_skill_layout(temporary.path());
    stage_knowledge_inspection_host(&binary);
    let pack = temporary.path().join("external-checkout");
    write_pack(&pack, "alpha", "External PACK");
    let mut command = knowledge_inspect_command(
        &binary,
        temporary.path(),
        &pack,
        "workflow",
        Some("cpu-time"),
    );
    command.env(
        "KAT_FAKE_RUNTIME_RESPONSE",
        r#"{"status":"success","result":{"workflow":{"name":"cpu-time","description":"Analyze CPU time.","parameters":[]}}}"#,
    );

    let output = command.output().expect("reject missing guide field");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        "PACK inspection Runtime failed"
    );
    assert!(response.get("result").is_none());
}

#[test]
fn targeted_pack_inspection_uses_adjacent_host_and_delivers_clean_log() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_minimum_skill_layout(temporary.path());
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
    assert_eq!(response["result"]["workflows"], serde_json::json!([]));
    assert_eq!(response["result"].as_object().unwrap().len(), 1);
    let log_path = PathBuf::from(response["log_path"].as_str().unwrap());
    let log = fs::read_to_string(log_path).expect("read Operation log");
    assert!(!log.contains('\u{1b}'));
    assert!(log.contains("runtime stdout\n"));
    assert!(log.contains("runtime stderr\n"));
    assert!(log.contains("invalid UTF-8 in Runtime stdout was replaced"));
    assert!(log.contains('\u{FFFD}'));
    assert!(log.contains("status: success\n"));
    assert_eq!(
        fs::read_to_string(pack.join("pack.toml")).unwrap(),
        manifest
    );
    assert_eq!(fs::read_dir(&pack).unwrap().count(), 1);
}

#[test]
#[ignore = "requires KAT_TEST_PYTHON and a wheel built from the current checkout"]
fn targeted_knowledge_inspection_runs_the_real_installed_host() {
    let python = PathBuf::from(
        std::env::var_os("KAT_TEST_PYTHON").expect("KAT_TEST_PYTHON identifies CPython"),
    );
    let workflow_wheel = PathBuf::from(
        std::env::var_os("KAT_TEST_WORKFLOW_WHEEL")
            .expect("KAT_TEST_WORKFLOW_WHEEL identifies the current wheel"),
    );
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) =
        support::stage_real_host_skill(temporary.path(), &cargo_kat(), &python, &workflow_wheel);
    let pack = temporary.path().join("external-checkout");
    write_pack(&pack, "alpha", "External PACK");
    let workflows = pack.join("workflows");
    fs::create_dir_all(&workflows).expect("create Workflow directory");
    fs::write(
        workflows.join("cpu.py"),
        r#"from kat import Context, workflow

@workflow(name="cpu-time", description="Analyze CPU time.", parameters={"limit": "Maximum rows"})
def analyze(ctx: Context, *, limit: int = 10):
    """Analyze CPU time."""
"#,
    )
    .expect("write Workflow entry");
    let datasources = pack.join("datasources");
    fs::create_dir_all(&datasources).expect("create Datasource directory");
    fs::write(
        datasources.join("postgresql.py"),
        r#"from kat import provider

@provider(name="postgresql", description="Query PostgreSQL.", guide="providers/postgresql.md")
class PostgreSQLProvider:
    pass
"#,
    )
    .expect("write Provider module");
    let knowledge = pack.join("knowledge").join("providers");
    fs::create_dir_all(&knowledge).expect("create Provider knowledge directory");
    fs::write(
        knowledge.join("postgresql.md"),
        "# PostgreSQL\n\nUse source SQL.\n",
    )
    .expect("write Provider guide");

    let mut command = Command::new(&binary);
    command
        .arg("inspect")
        .arg("workflow")
        .args(["--pack", "alpha"])
        .args(["--workflow", "cpu-time"])
        .arg("--pack-dir")
        .arg(&pack);
    prepare_platform_data_home(&mut command, temporary.path());
    let output = command.output().expect("inspect with real Workflow Host");

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "success");
    assert_eq!(response["result"]["workflow"]["name"], "cpu-time");
    assert_eq!(
        response["result"]["workflow"]["parameters"][0]["default"],
        "10"
    );
    assert!(!pack.join("workflows").join("__pycache__").exists());

    let mut provider_command = Command::new(&binary);
    provider_command
        .arg("inspect")
        .arg("provider")
        .args(["--pack", "alpha"])
        .args(["--provider", "postgresql"])
        .arg("--pack-dir")
        .arg(&pack);
    prepare_platform_data_home(&mut provider_command, temporary.path());
    let provider_output = provider_command
        .output()
        .expect("inspect with real Provider Host");

    assert_eq!(
        provider_output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&provider_output.stderr)
    );
    let provider_response: serde_json::Value =
        serde_json::from_slice(&provider_output.stdout).unwrap();
    assert_eq!(
        provider_response["result"]["provider"],
        serde_json::json!({
            "name": "postgresql",
            "description": "Query PostgreSQL.",
            "module": "kat.pack.datasources.postgresql",
            "qualname": "PostgreSQLProvider",
            "guide": "# PostgreSQL\n\nUse source SQL.\n"
        })
    );
    assert!(!pack.join("datasources").join("__pycache__").exists());

    fs::write(
        workflows.join("cpu.py"),
        r#"from kat import Context, workflow

class MetadataProxy:
    def __getattribute__(self, attribute):
        if attribute == "__module__":
            raise RuntimeError("author metadata must not execute")
        return object.__getattribute__(self, attribute)

proxy = MetadataProxy()

def invalid_default():
    raise RuntimeError("author default failed")

@workflow(name="cpu-time", description="Analyze CPU time.", parameters={"limit": "Maximum rows"})
def analyze(ctx: Context, *, limit: int = invalid_default):
    """Analyze CPU time."""
"#,
    )
    .expect("replace Workflow entry with an author failure");

    let mut command = targeted_inspect_command(&binary, temporary.path(), &pack);
    let output = command
        .output()
        .expect("inspect author failure with real Workflow Host");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert_eq!(response["error"]["message"], "PACK inspection failed");
    assert!(response.get("result").is_none());
    assert!(response.get("failure_owner").is_none());
    let log_path = PathBuf::from(
        response["log_path"]
            .as_str()
            .expect("readable Operation log"),
    );
    assert!(log_path.is_file());
}

#[test]
fn targeted_pack_inspection_enforces_runtime_exit_and_response_matrix() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_minimum_skill_layout(temporary.path());
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
#[cfg(not(windows))]
fn targeted_pack_inspection_log_creation_failure_does_not_start_runtime() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_minimum_skill_layout(temporary.path());
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
    let (_skill, binary) = stage_minimum_skill_layout(temporary.path());
    let pack = temporary.path().join("external-checkout");
    write_pack(&pack, "alpha", "External PACK");
    let mut command = Command::new(&binary);
    command
        .arg("inspect")
        .arg("workflow")
        .args(["--pack", "missing"])
        .arg("--pack-dir")
        .arg(&pack);
    prepare_platform_data_home(&mut command, temporary.path());

    let output = command.output().expect("reject unknown PACK");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response["error"]["message"],
        "PACK \"missing\" was not discovered"
    );
    assert!(response.get("result").is_none());
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert!(log.contains("operation: kat inspect workflow"));
    assert!(log.contains("pack: missing"));
    assert!(log.contains("status: failure"));

    let duplicate = temporary.path().join("duplicate-checkout");
    write_pack(&duplicate, "alpha", "Duplicate PACK");
    let mut duplicate_command = Command::new(&binary);
    duplicate_command
        .arg("inspect")
        .arg("workflow")
        .args(["--pack", "alpha"])
        .arg("--pack-dir")
        .arg(&pack)
        .arg("--pack-dir")
        .arg(&duplicate);
    prepare_platform_data_home(&mut duplicate_command, temporary.path());

    let duplicate_output = duplicate_command
        .output()
        .expect("reject duplicate PACK name");

    assert_eq!(duplicate_output.status.code(), Some(1));
    let duplicate_response: serde_json::Value =
        serde_json::from_slice(&duplicate_output.stdout).unwrap();
    assert_eq!(
        duplicate_response["error"]["help"],
        "Remove one conflicting PACK or give the PACKs distinct names, then retry"
    );
    let duplicate_log =
        fs::read_to_string(duplicate_response["log_path"].as_str().unwrap()).unwrap();
    assert!(duplicate_log.contains("status: failure"));
    #[cfg(not(windows))]
    assert_eq!(
        fs::read_dir(data_home(temporary.path()).join("logs"))
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn targeted_pack_log_header_escapes_untrusted_pack_name() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_minimum_skill_layout(temporary.path());
    let mut command = Command::new(binary);
    command
        .arg("inspect")
        .arg("workflow")
        .arg("--pack")
        .arg("bad\nforged:\x1b[31mred\r")
        .env(
            "KAT_FAKE_RUNTIME_RESPONSE",
            r#"{"status":"success","result":{"workflows":[]}}"#,
        );
    prepare_platform_data_home(&mut command, temporary.path());

    let output = command.output().expect("reject untrusted PACK name");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let log = fs::read_to_string(response["log_path"].as_str().unwrap()).unwrap();
    assert!(!log.contains('\x1b'));
    assert!(!log.contains('\r'));
    assert!(log.contains("pack: bad\\nforged:"));
    assert!(log.contains("error: PACK \"bad\\nforged:"));
    assert!(!log.lines().any(|line| line.starts_with("forged:")));
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
    let operation_help_text = String::from_utf8(operation_help.stdout).expect("UTF-8 help");
    assert!(operation_help_text.contains("Workflow and Provider knowledge"));
    assert!(operation_help_text.contains("workflow"));
    assert!(operation_help_text.contains("provider"));
    assert!(operation_help_text.contains("validation order"));
    assert!(operation_help_text.contains("sorted by PACK name"));

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
#[cfg_attr(
    windows,
    ignore = "requires a clean Windows user profile; full-ci runs it on windows-latest"
)]
fn manifest_only_inspection_never_imports_pack_python() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_minimum_skill_layout(temporary.path());
    let pack = temporary.path().join("external-checkout");
    write_pack(&pack, "alpha", "External PACK");
    let datasources = pack.join("datasources");
    fs::create_dir_all(&datasources).expect("create Datasource directory");
    let marker = temporary.path().join("python-imported");
    fs::write(
        datasources.join("broken.py"),
        format!(
            "from pathlib import Path\nPath({:?}).write_text('imported')\nraise RuntimeError('must not import')\n",
            marker
        ),
    )
    .expect("write hostile Datasource module");
    let mut command = Command::new(binary);
    command.arg("inspect").arg("--pack-dir").arg(&pack);
    prepare_platform_data_home(&mut command, temporary.path());

    let output = command.output().expect("inspect manifests only");

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        response["result"]["packs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|candidate| candidate["name"] == "alpha")
    );
    assert!(response.get("log_path").is_none());
    assert!(!marker.exists());
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires a clean Windows user profile; full-ci runs it on windows-latest"
)]
fn inspect_lists_all_packs_from_a_moved_skill_and_arbitrary_cwd() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (skill, binary) = stage_minimum_skill_layout(temporary.path());
    let bundled = skill.join("assets").join("packs").join("bundled-directory");
    let bundled_manifest = write_pack(&bundled, "bravo", "Bundled description");
    let cwd = temporary.path().join("unrelated-cwd");
    let additional = cwd.join("relative-pack");
    let additional_manifest = write_pack(&additional, "alpha", "External description");
    #[cfg(not(windows))]
    let (data_pack, data_manifest) = {
        let data_pack = data_home(temporary.path())
            .join("packs")
            .join("data-directory");
        let data_manifest = write_pack(&data_pack, "charlie", "Data Home description");
        (data_pack, data_manifest)
    };
    let moved_skill = temporary.path().join("moved-again");
    fs::rename(&skill, &moved_skill).expect("move minimum Skill layout");
    let moved_binary = moved_skill.join(binary.strip_prefix(&skill).unwrap());
    let moved_bundled = moved_skill.join(bundled.strip_prefix(&skill).unwrap());

    let mut command = Command::new(moved_binary);
    command
        .current_dir(&cwd)
        .args(["inspect", "--pack-dir", "relative-pack"]);
    prepare_platform_data_home(&mut command, temporary.path());
    let output = command.output().expect("run staged kat inspect");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    #[cfg(not(windows))]
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"status\":\"success\",\"result\":{\"packs\":[{\"name\":\"alpha\",\"title\":\"alpha\",\"description\":\"External description\",\"owner\":\"Test Team\"},{\"name\":\"bravo\",\"title\":\"bravo\",\"description\":\"Bundled description\",\"owner\":\"Test Team\"},{\"name\":\"charlie\",\"title\":\"charlie\",\"description\":\"Data Home description\",\"owner\":\"Test Team\"}]}}\n"
    );
    #[cfg(windows)]
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"status\":\"success\",\"result\":{\"packs\":[{\"name\":\"alpha\",\"title\":\"alpha\",\"description\":\"External description\",\"owner\":\"Test Team\"},{\"name\":\"bravo\",\"title\":\"bravo\",\"description\":\"Bundled description\",\"owner\":\"Test Team\"}]}}\n"
    );
    assert_eq!(
        fs::read_to_string(moved_bundled.join("pack.toml")).unwrap(),
        bundled_manifest
    );
    assert_eq!(
        fs::read_to_string(additional.join("pack.toml")).unwrap(),
        additional_manifest
    );
    #[cfg(not(windows))]
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
    #[cfg(not(windows))]
    assert!(!data_home(temporary.path()).join("logs").exists());
}

#[test]
fn formed_operation_rejects_a_binary_outside_the_minimum_skill_layout() {
    let mut command = Command::new(cargo_kat());
    command.arg("inspect");

    let output = command.output().expect("run unstaged kat inspect");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert_eq!(response["error"]["message"], "KAT Skill is unavailable");
    assert!(response.get("result").is_none());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("KAT Skill is unavailable"));
    assert!(diagnostic.contains("<skill>/scripts/targets/<target>"));
    assert!(diagnostic.contains("regular <skill>/"));
    assert!(diagnostic.contains("SKILL.md marker"));
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires a clean Windows user profile; full-ci runs it on windows-latest"
)]
fn formed_operation_failure_is_one_json_response_and_readable_diagnostic() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_minimum_skill_layout(temporary.path());
    let broken = temporary.path().join("broken-pack");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join("pack.toml"), "not valid TOML = [").unwrap();
    let mut command = Command::new(binary);
    command.arg("inspect").arg("--pack-dir").arg(&broken);
    prepare_platform_data_home(&mut command, temporary.path());

    let output = command.output().expect("run failed inspection");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).expect("failure JSON");
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    assert!(response.get("log_path").is_none());
    assert_eq!(response["error"]["message"], "PACK discovery failed");
    assert!(response["error"]["causes"].as_array().unwrap().len() >= 2);
    assert!(String::from_utf8_lossy(&output.stderr).contains("PACK discovery failed"));
    #[cfg(not(windows))]
    assert!(!data_home(temporary.path()).join("logs").exists());
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires a clean Windows user profile; full-ci runs it on windows-latest"
)]
fn duplicate_pack_names_report_conflict_specific_help() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (_skill, binary) = stage_minimum_skill_layout(temporary.path());
    let first = temporary.path().join("first-pack");
    let second = temporary.path().join("second-pack");
    write_pack(&first, "duplicate", "First description");
    write_pack(&second, "duplicate", "Second description");
    let mut command = Command::new(binary);
    command
        .arg("inspect")
        .arg("--pack-dir")
        .arg(&first)
        .arg("--pack-dir")
        .arg(&second);
    prepare_platform_data_home(&mut command, temporary.path());

    let output = command.output().expect("run duplicate PACK inspection");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).expect("failure JSON");
    assert_eq!(response["status"], "failure");
    assert_eq!(
        response["error"]["help"],
        "Remove one conflicting PACK or give the PACKs distinct names, then retry"
    );
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("Remove one conflicting PACK"));
    assert!(diagnostic.contains("distinct names"));
    assert!(response.get("result").is_none());
    assert!(response.get("log_path").is_none());
}

#[test]
fn invalid_default_pack_search_path_reports_search_specific_help() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (skill, binary) = stage_minimum_skill_layout(temporary.path());
    let assets = skill.join("assets");
    fs::create_dir_all(&assets).expect("create Skill assets directory");
    fs::write(assets.join("packs"), "not a directory")
        .expect("create invalid default PACK search path");
    let mut command = Command::new(binary);
    command.arg("inspect");

    let output = command
        .output()
        .expect("run invalid default PACK search inspection");

    assert_eq!(output.status.code(), Some(1));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).expect("failure JSON");
    assert_eq!(response["status"], "failure");
    assert_eq!(
        response["error"]["help"],
        "Make the default PACK search path a readable directory or remove it, then retry"
    );
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("default PACK search path"));
    assert!(diagnostic.contains("readable directory"));
    assert!(response.get("result").is_none());
    assert!(response.get("log_path").is_none());
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires a clean Windows user profile; full-ci runs it on windows-latest"
)]
fn absent_default_directories_are_an_empty_result_and_are_not_created() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (skill, binary) = stage_minimum_skill_layout(temporary.path());
    let mut command = Command::new(binary);
    command.arg("inspect");
    prepare_platform_data_home(&mut command, temporary.path());

    let output = command.output().expect("run empty inspection");

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"status\":\"success\",\"result\":{\"packs\":[]}}\n"
    );
    assert!(!skill.join("assets").join("packs").exists());
    #[cfg(not(windows))]
    assert!(!data_home(temporary.path()).exists());
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires a clean Windows user profile; full-ci runs it on windows-latest"
)]
fn closed_stdout_makes_the_real_process_fail() {
    let temporary = tempfile::tempdir().expect("create temporary directory");
    let (skill, binary) = stage_minimum_skill_layout(temporary.path());
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
    prepare_platform_data_home(&mut command, temporary.path());
    let mut child = command.spawn().expect("spawn kat inspect");
    drop(child.stdout.take());

    let output = child.wait_with_output().expect("wait for kat inspect");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("write KAT Response"));
}
#[test]
fn inspect_dataset_option_is_not_a_cli_surface() {
    let help = Command::new(cargo_kat())
        .args(["inspect", "--help"])
        .output()
        .unwrap();
    assert_eq!(help.status.code(), Some(0));
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(!help.contains("Dataset"));
    assert!(!help.contains("--dataset"));

    let removed = Command::new(cargo_kat())
        .args(["inspect", "--dataset", "legacy-dataset"])
        .output()
        .unwrap();
    assert_eq!(removed.status.code(), Some(2));
    assert!(removed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&removed.stderr).contains("unexpected argument '--dataset'"));
}
