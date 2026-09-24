use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

#[path = "support/parquet.rs"]
mod parquet_fixture;
mod support;

const SESSION: &str = "019f6e00-0000-7000-8000-000000000398";
const RUN: &str = "019f6e00-0000-7000-8000-000000000399";

struct Fixture {
    root: tempfile::TempDir,
    binary: PathBuf,
    data: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let (_, binary) = support::stage_skill(root.path(), "skill");
        let data = root.path().join("data");
        let session = data.join("sessions").join(SESSION);
        fs::create_dir_all(data.join("sessions/.leases")).unwrap();
        fs::create_dir_all(data.join("sessions/.deletions")).unwrap();
        fs::write(
            data.join("sessions/.leases")
                .join(format!("{SESSION}.lock")),
            [],
        )
        .unwrap();
        fs::create_dir_all(session.join("materializations")).unwrap();
        fs::create_dir_all(session.join("scratch")).unwrap();
        let run = session.join("runs").join(RUN);
        fs::create_dir_all(run.join("outputs")).unwrap();
        fs::write(
            session.join("session.json"),
            serde_json::to_vec(&serde_json::json!({"session_id":SESSION})).unwrap(),
        )
        .unwrap();
        parquet_fixture::write_i64(&run.join("outputs/main.parquet"), "value", &[1]);
        fs::write(run.join("manifest.json"), serde_json::to_vec(&serde_json::json!({
            "session_id":SESSION,"run_id":RUN,"pack":"alpha","workflow":"analyze","guide":null,
            "child_runs":[],"inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":1}}
        })).unwrap()).unwrap();
        stage_host(&binary);
        Self { root, binary, data }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        command.env("KAT_DATA_HOME", &self.data);
        command.env(
            "KAT_ARCHIVE_RUNTIME_CALLED",
            self.root.path().join("runtime-called"),
        );
        command
    }

    fn init(&self) {
        let goal = self.root.path().join("goal.json");
        fs::write(
            &goal,
            r#"{"goal":{"question":"Locate the bottleneck","scope":"this trace","gaps":[]}}"#,
        )
        .unwrap();
        success(
            self.command()
                .args([
                    "analysis",
                    "save",
                    "--session",
                    SESSION,
                    "--expected-revision",
                    "0",
                    "--file",
                ])
                .arg(goal)
                .output()
                .unwrap(),
        );
    }

    fn query(&self, sql: &str, ndjson: &str) -> Command {
        let mut command = self.command();
        command
            .args(["query", "--session", SESSION, "--run", RUN, "--sql", sql])
            .env("KAT_ARCHIVE_ROWS", ndjson)
            .env(
                "KAT_ARCHIVE_RUNTIME_RESPONSE",
                r#"{"status":"success","result":{"columns":[{"name":"value","type":"int64"}]}}"#,
            );
        command
    }

    fn material(&self, material: &str) -> serde_json::Value {
        success(
            self.command()
                .args([
                    "analysis",
                    "show",
                    "--session",
                    SESSION,
                    "--material",
                    material,
                ])
                .output()
                .unwrap(),
        )
    }
}

fn success(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "success", "{response}");
    response["result"].clone()
}

fn failure(output: Output) -> serde_json::Value {
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure", "{response}");
    response
}

#[test]
fn query_archives_original_ndjson_and_sql_without_numeric_reencoding() {
    let fixture = Fixture::new();
    fixture.init();
    let sql = "SELECT\n value FROM output.main";
    let rows = "{\"value\":9223372036854775807}\r\n{\"value\":-9223372036854775808}\n";
    let ordinary = success(fixture.query(sql, rows).output().unwrap());
    assert!(ordinary.get("analysis").is_none());
    let saved_path = fixture.root.path().join("saved-report.json");
    fs::write(
        &saved_path,
        serde_json::to_vec(&serde_json::json!({
            "nodes": [{"run_id": RUN, "content": "Existing explanation"}],
            "report": {"content": "Previously saved report"}
        }))
        .unwrap(),
    )
    .unwrap();
    let saved = success(
        fixture
            .command()
            .args([
                "analysis",
                "save",
                "--session",
                SESSION,
                "--expected-revision",
                "1",
                "--file",
            ])
            .arg(saved_path)
            .output()
            .unwrap(),
    );
    let archived = success(fixture.query(sql, rows).arg("--archive").output().unwrap());
    assert_eq!(archived["analysis"]["previous_revision"], 2);
    assert_eq!(archived["analysis"]["revision"], 3);
    let restored = success(
        fixture
            .command()
            .args(["analysis", "show", "--session", SESSION])
            .output()
            .unwrap(),
    );
    assert_eq!(restored["nodes"], saved["nodes"]);
    assert_eq!(restored["report"], saved["report"]);
    assert_eq!(
        fs::read_to_string(archived["path"].as_str().unwrap()).unwrap(),
        rows
    );
    fs::remove_file(archived["path"].as_str().unwrap()).unwrap();
    let material = fixture.material(archived["analysis"]["material_id"].as_str().unwrap());
    assert_eq!(material["material"]["kind"], "query");
    assert_eq!(material["material"]["run_id"], RUN);
    assert_eq!(material["material"]["sql"], sql);
    assert_eq!(material["material"]["ndjson"], rows);
    assert_eq!(material["material"]["columns"], archived["columns"]);
    assert_eq!(material["material"]["outputs"], serde_json::json!(["main"]));
    assert!(!material.to_string().contains(".parquet"));
}

#[test]
fn archive_failure_after_query_keeps_previous_record_and_reports_partial_success() {
    let fixture = Fixture::new();
    fixture.init();
    let analysis = fixture.data.join("sessions").join(SESSION).join("analysis");
    let previous = fs::read(analysis.join("record.json")).unwrap();
    let failed = failure(
        fixture
            .query("SELECT 1", "{\"value\":1}\n")
            .arg("--archive")
            .env("KAT_ARCHIVE_BLOCK_LOCK", analysis.join("write.lock"))
            .output()
            .unwrap(),
    );
    assert!(
        failed["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Query succeeded"),
        "{failed}"
    );
    assert!(!failed.to_string().contains("material_id"));
    assert_eq!(fs::read(analysis.join("record.json")).unwrap(), previous);
}

#[test]
fn lost_query_response_does_not_roll_back_committed_evidence() {
    let fixture = Fixture::new();
    fixture.init();
    let rows = "{\"value\":9223372036854775807}\n";
    let mut child = fixture
        .query("SELECT value FROM output.main", rows)
        .arg("--archive")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    assert!(!child.wait_with_output().unwrap().status.success());
    let restored = success(
        fixture
            .command()
            .args(["analysis", "show", "--session", SESSION])
            .output()
            .unwrap(),
    );
    assert_eq!(restored["revision"], 2);
    let materials = restored["materials"].as_object().unwrap();
    assert_eq!(materials.len(), 1);
    let material = fixture.material(materials.keys().next().unwrap());
    assert_eq!(material["material"]["ndjson"], rows);
}

#[test]
fn concurrent_archives_append_to_latest_revision_without_lost_materials() {
    let fixture = Fixture::new();
    fixture.init();
    let ready = fixture.root.path().join("ready");
    fs::create_dir(&ready).unwrap();
    let release = fixture.root.path().join("release");
    let mut children = Vec::new();
    for value in [1, 2] {
        children.push(
            fixture
                .query(
                    "SELECT value FROM output.main",
                    &format!("{{\"value\":{value}}}\n"),
                )
                .arg("--archive")
                .env("KAT_ARCHIVE_READY", ready.join(value.to_string()))
                .env("KAT_ARCHIVE_RELEASE", &release)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    while fs::read_dir(&ready).unwrap().count() != 2 {
        assert!(
            Instant::now() < deadline,
            "both archive runtimes must reach the barrier"
        );
        thread::sleep(Duration::from_millis(10));
    }
    fs::write(release, []).unwrap();
    let mut results = children
        .into_iter()
        .map(|child| success(child.wait_with_output().unwrap()))
        .collect::<Vec<_>>();
    results.sort_by_key(|result| result["analysis"]["revision"].as_u64().unwrap());
    for (index, result) in results.iter().enumerate() {
        assert_eq!(result["analysis"]["previous_revision"], 1 + index as u64);
        assert_eq!(result["analysis"]["revision"], 2 + index as u64);
        let material = fixture.material(result["analysis"]["material_id"].as_str().unwrap());
        assert_eq!(
            material["material"]["ndjson"],
            fs::read_to_string(result["path"].as_str().unwrap()).unwrap()
        );
    }
    assert_ne!(
        results[0]["analysis"]["material_id"],
        results[1]["analysis"]["material_id"]
    );
}

#[test]
fn archiving_requires_existing_analysis_before_invoking_runtime() {
    let fixture = Fixture::new();
    failure(
        fixture
            .query("SELECT 1", "{\"value\":1}\n")
            .arg("--archive")
            .output()
            .unwrap(),
    );
    assert!(!fixture.root.path().join("runtime-called").exists());
}

fn stage_host(binary: &Path) {
    let host = support::host_path(binary);
    fs::create_dir_all(host.parent().unwrap()).unwrap();
    let source = binary.parent().unwrap().join("archive-host.rs");
    fs::write(
        &source,
        r#"
use std::{env, fs, path::Path, thread, time::Duration};
fn field(document: &str, field: &str) -> String {
    let prefix = format!("\"{field}\":\"");
    let mut characters = document.split_once(&prefix).unwrap().1.chars();
    let mut value = String::new();
    while let Some(character) = characters.next() {
        match character {
            '"' => return value,
            '\\' => match characters.next().unwrap() {
                '"' => value.push('"'), '\\' => value.push('\\'), '/' => value.push('/'),
                'n' => value.push('\n'), 'r' => value.push('\r'), 't' => value.push('\t'),
                _ => panic!("unexpected path escape"),
            },
            character => value.push(character),
        }
    }
    panic!("unterminated value")
}
fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.get(4).map(String::as_str) == Some("-c") {
        let location = env::current_exe().unwrap().parent().unwrap().join("sdk-fixture");
        print!("{:?}", location.to_str().unwrap());
        return;
    }
    assert_eq!(args.len(), 11);
    let request = fs::read_to_string(&args[8]).unwrap();
    fs::write(env::var_os("KAT_ARCHIVE_RUNTIME_CALLED").unwrap(), &request).unwrap();
    if request.contains("\"operation\":\"query_run\"") {
        fs::write(field(&request, "result_path"), env::var("KAT_ARCHIVE_ROWS").unwrap()).unwrap();
    }
    if let Some(ready) = env::var_os("KAT_ARCHIVE_READY") {
        fs::write(ready, []).unwrap();
        let release = env::var_os("KAT_ARCHIVE_RELEASE").unwrap();
        while !Path::new(&release).exists() { thread::sleep(Duration::from_millis(10)); }
    }
    if let Some(lock) = env::var_os("KAT_ARCHIVE_BLOCK_LOCK") {
        fs::remove_file(&lock).unwrap();
        fs::create_dir(lock).unwrap();
    }
    fs::write(&args[10], env::var("KAT_ARCHIVE_RUNTIME_RESPONSE").unwrap()).unwrap();
}
"#,
    )
    .unwrap();
    let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
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
