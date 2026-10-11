use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

use serde_json::{Value, json};

mod support;

const ORIGINAL: &str = "---\r\nname: code-review\r\ndescription: 按团队准则评审代码\r\n---\r\n\r\n检查[准则](../../knowledge/rules.md)，交付问题与证据。\r\n";

struct Fixture {
    _root: tempfile::TempDir,
    binary: PathBuf,
    home: PathBuf,
    pack: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let (_, binary) = support::stage_skill(root.path(), "kat");
        fs::remove_file(support::host_path(&binary)).unwrap();
        let home = root.path().join("data");
        fs::create_dir_all(&home).unwrap();
        let pack = home.join("packs/review-pack");
        fs::create_dir_all(pack.join("skills/code-review")).unwrap();
        fs::write(pack.join("pack.toml"), "name = \"review-pack\"\ntitle = \"Review\"\ndescription = \"团队评审经验\"\nowner = \"Review team\"\n").unwrap();
        fs::write(pack.join("skills/code-review/SKILL.md"), ORIGINAL).unwrap();
        Self {
            _root: root,
            binary,
            home,
            pack,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        command.env("KAT_DATA_HOME", &self.home);
        command
    }

    fn session(&self) -> String {
        let created = success(self.command().args(["session", "create"]).output().unwrap());
        let session = created["session_id"].as_str().unwrap().to_owned();
        let input = self.home.join("goal.json");
        fs::write(
            &input,
            r#"{"goal":{"question":"评审给定变更","scope":"受控材料","gaps":[]}}"#,
        )
        .unwrap();
        success(
            self.command()
                .args([
                    "analysis",
                    "save",
                    "--session",
                    &session,
                    "--expected-revision",
                    "0",
                    "--file",
                ])
                .arg(&input)
                .output()
                .unwrap(),
        );
        session
    }
}

#[test]
fn adopted_knowledge_and_result_recover_after_pack_removal_without_runs() {
    let fixture = Fixture::new();
    let session = fixture.session();
    let adopted = success(
        fixture
            .command()
            .args([
                "knowledge",
                "read",
                "--session",
                &session,
                "--pack",
                "review-pack",
                "--entry",
                "code-review",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(adopted["material"]["content"], ORIGINAL);
    assert_eq!(adopted["previous_revision"], 1);
    assert_eq!(adopted["revision"], 2);
    let id = adopted["material_id"].as_str().unwrap();
    let input = fixture.home.join("report.json");
    fs::write(
        &input,
        serde_json::to_vec(
            &json!({"report":{"content":format!("按材料 {id} 评审：缺少失败边界验证。")}}),
        )
        .unwrap(),
    )
    .unwrap();
    success(
        fixture
            .command()
            .args([
                "analysis",
                "save",
                "--session",
                &session,
                "--expected-revision",
                "2",
                "--file",
            ])
            .arg(&input)
            .output()
            .unwrap(),
    );
    fs::rename(&fixture.pack, fixture.home.join("removed-pack")).unwrap();
    let recovered = success(
        fixture
            .command()
            .args([
                "knowledge",
                "read",
                "--session",
                &session,
                "--pack",
                "review-pack",
                "--entry",
                "code-review",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(recovered["material"], adopted["material"]);
    assert_eq!(recovered["material_id"], id);
    assert_eq!(recovered["previous_revision"], 3);
    assert_eq!(recovered["revision"], 3);
    let material = success(
        fixture
            .command()
            .args(["analysis", "show", "--session", &session, "--material", id])
            .output()
            .unwrap(),
    );
    assert_eq!(material["material"], adopted["material"]);
    let record = success(
        fixture
            .command()
            .args(["analysis", "show", "--session", &session])
            .output()
            .unwrap(),
    );
    assert_eq!(record["nodes"], json!([]));
    assert!(record["report"]["content"].as_str().unwrap().contains(id));
    let inventory = success(
        fixture
            .command()
            .args(["inspect", "session", "--session", &session])
            .output()
            .unwrap(),
    );
    assert_eq!(inventory["runs"], json!([]));
}

fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "success");
    response["result"].clone()
}

fn failure(output: Output) -> Value {
    assert!(!output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failure");
    assert!(response.get("result").is_none());
    response["error"].clone()
}

#[test]
fn explicit_update_retains_old_methods_and_requires_material_selection() {
    let fixture = Fixture::new();
    let session = fixture.session();
    let arguments = [
        "knowledge",
        "read",
        "--session",
        &session,
        "--pack",
        "review-pack",
        "--entry",
        "code-review",
    ];
    let first = success(fixture.command().args(arguments).output().unwrap());
    let newer = ORIGINAL.replace("交付问题与证据。", "先核对授权，再交付问题与证据。");
    fs::write(fixture.pack.join("skills/code-review/SKILL.md"), &newer).unwrap();
    let unchanged = success(fixture.command().args(arguments).output().unwrap());
    assert_eq!(unchanged["material_id"], first["material_id"]);
    assert_eq!(unchanged["material"]["content"], ORIGINAL);
    let updated = success(
        fixture
            .command()
            .args(arguments)
            .args(["--update", "--expected-revision", "2"])
            .output()
            .unwrap(),
    );
    assert_eq!(updated["material"]["content"], newer);
    assert_ne!(updated["material_id"], first["material_id"]);
    assert_eq!(updated["revision"], 3);
    let conflict = failure(
        fixture
            .command()
            .args(arguments)
            .args(["--update", "--expected-revision", "2"])
            .output()
            .unwrap(),
    );
    assert!(
        conflict["message"]
            .as_str()
            .unwrap()
            .contains("revision conflict")
    );
    let ambiguous = failure(fixture.command().args(arguments).output().unwrap());
    let message = ambiguous["message"].as_str().unwrap();
    assert!(message.contains(first["material_id"].as_str().unwrap()));
    assert!(message.contains(updated["material_id"].as_str().unwrap()));
    for original in [&first, &updated] {
        let read = success(
            fixture
                .command()
                .args([
                    "knowledge",
                    "read",
                    "--session",
                    &session,
                    "--material",
                    original["material_id"].as_str().unwrap(),
                ])
                .output()
                .unwrap(),
        );
        assert_eq!(read["material"], original["material"]);
        assert_eq!(read["revision"], 3);
    }
}

#[test]
fn pure_knowledge_pack_is_discovered_and_browsed_without_python_or_session() {
    let fixture = Fixture::new();
    let list = success(
        fixture
            .command()
            .args(["inspect", "knowledge", "--pack", "review-pack"])
            .output()
            .unwrap(),
    );
    assert_eq!(
        list,
        json!({"entries":[{"name":"code-review","description":"按团队准则评审代码"}]})
    );
    let entry = success(
        fixture
            .command()
            .args([
                "inspect",
                "knowledge",
                "--pack",
                "review-pack",
                "--entry",
                "code-review",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(
        entry,
        json!({"entry":{"name":"code-review","description":"按团队准则评审代码","path":"skills/code-review/SKILL.md","content":ORIGINAL}})
    );
    assert!(!fixture.home.join("sessions").exists());
}

#[test]
fn reference_selectors_cannot_bypass_entry_validation_or_escape_the_pack() {
    let fixture = Fixture::new();
    let session = fixture.session();
    for path in [
        "skills/code-review/SKILL.md ",
        "skills/code-review/SKILL.md.",
        "skills/code-review/SKILL.md",
        "../outside.md",
        "knowledge//rules.md",
        "knowledge\\rules.md",
    ] {
        failure(
            fixture
                .command()
                .args([
                    "knowledge",
                    "read",
                    "--session",
                    &session,
                    "--pack",
                    "review-pack",
                    "--path",
                    path,
                ])
                .output()
                .unwrap(),
        );
    }
    let record = success(
        fixture
            .command()
            .args(["analysis", "show", "--session", &session])
            .output()
            .unwrap(),
    );
    assert_eq!(record["revision"], 1);
    assert_eq!(record["materials"], json!({}));
}

#[test]
fn declarations_are_strict_and_listing_does_not_read_unrequested_references() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.pack.join("knowledge")).unwrap();
    fs::write(fixture.pack.join("knowledge/rules.md"), [0xff, 0]).unwrap();
    success(
        fixture
            .command()
            .args(["inspect", "knowledge", "--pack", "review-pack"])
            .output()
            .unwrap(),
    );
    for invalid in [
        "---\nname: code-review\ndescription: 123\n---\nBody",
        "---\nname: code-review\nname: code-review\ndescription: Valid\n---\nBody",
        "---\nname: code-review\ndescription: Valid\nextra: ignored\n---\nBody",
        "---\nname: other\ndescription: Valid\n---\nBody",
        "---\nname: code-review\ndescription: '  '\n---\nBody",
        "---\nname: code-review\ndescription: Valid\n---\n \n",
        "---\nname: code-review\ndescription: Valid\n",
    ] {
        fs::write(fixture.pack.join("skills/code-review/SKILL.md"), invalid).unwrap();
        failure(
            fixture
                .command()
                .args(["inspect", "knowledge", "--pack", "review-pack"])
                .output()
                .unwrap(),
        );
        failure(
            fixture
                .command()
                .args([
                    "inspect",
                    "knowledge",
                    "--pack",
                    "review-pack",
                    "--entry",
                    "code-review",
                ])
                .output()
                .unwrap(),
        );
    }
    fs::write(fixture.pack.join("skills/code-review/SKILL.md"), ORIGINAL).unwrap();
    fs::create_dir_all(fixture.pack.join("skills/bad-entry")).unwrap();
    fs::write(fixture.pack.join("skills/bad-entry/SKILL.md"), "invalid").unwrap();
    failure(
        fixture
            .command()
            .args(["inspect", "knowledge", "--pack", "review-pack"])
            .output()
            .unwrap(),
    );
    success(
        fixture
            .command()
            .args([
                "inspect",
                "knowledge",
                "--pack",
                "review-pack",
                "--entry",
                "code-review",
            ])
            .output()
            .unwrap(),
    );
}

#[test]
fn references_are_lazy_immutable_text_materials_and_failures_do_not_write() {
    let fixture = Fixture::new();
    let session = fixture.session();
    fs::create_dir_all(fixture.pack.join("knowledge")).unwrap();
    let path = fixture.pack.join("knowledge/rules.md");
    fs::write(&path, "# 准则\r\n只报告有证据的问题。\r\n").unwrap();
    let args = [
        "knowledge",
        "read",
        "--session",
        &session,
        "--pack",
        "review-pack",
        "--path",
        "knowledge/rules.md",
    ];
    let first = success(fixture.command().args(args).output().unwrap());
    assert_eq!(first["material"]["entry"], Value::Null);
    fs::write(&path, "Changed").unwrap();
    let cached = success(fixture.command().args(args).output().unwrap());
    assert_eq!(cached["material"], first["material"]);
    let updated = success(
        fixture
            .command()
            .args(args)
            .args(["--update", "--expected-revision", "2"])
            .output()
            .unwrap(),
    );
    assert_eq!(updated["material"]["content"], "Changed");
    failure(fixture.command().args(args).output().unwrap());
    for (name, bytes) in [
        ("bad.txt", vec![0xff]),
        ("nul.txt", vec![0]),
        ("sample.pdf", b"%PDF-1.7".to_vec()),
    ] {
        fs::write(fixture.pack.join("knowledge").join(name), bytes).unwrap();
        failure(
            fixture
                .command()
                .args([
                    "knowledge",
                    "read",
                    "--session",
                    &session,
                    "--pack",
                    "review-pack",
                    "--path",
                    &format!("knowledge/{name}"),
                ])
                .output()
                .unwrap(),
        );
    }
    failure(
        fixture
            .command()
            .args([
                "knowledge",
                "read",
                "--session",
                &session,
                "--pack",
                "review-pack",
                "--path",
                "knowledge/missing.txt",
            ])
            .output()
            .unwrap(),
    );
    let record = success(
        fixture
            .command()
            .args(["analysis", "show", "--session", &session])
            .output()
            .unwrap(),
    );
    assert_eq!(record["revision"], 3);
    assert_eq!(record["materials"].as_object().unwrap().len(), 2);
    assert!(
        record["materials"]
            .as_object()
            .unwrap()
            .values()
            .all(|value| value.get("content").is_none())
    );
}

#[test]
fn concurrent_first_adoption_deduplicates_under_the_record_lock() {
    let fixture = Fixture::new();
    let session = fixture.session();
    let children: Vec<_> = (0..6)
        .map(|_| {
            fixture
                .command()
                .args([
                    "knowledge",
                    "read",
                    "--session",
                    &session,
                    "--pack",
                    "review-pack",
                    "--entry",
                    "code-review",
                ])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let receipts: Vec<_> = children
        .into_iter()
        .map(|child| success(child.wait_with_output().unwrap()))
        .collect();
    assert!(receipts.iter().all(
        |receipt| receipt["material_id"] == receipts[0]["material_id"] && receipt["revision"] == 2
    ));
    assert_eq!(
        receipts
            .iter()
            .filter(|receipt| receipt["previous_revision"] == 1)
            .count(),
        1
    );
    let record = success(
        fixture
            .command()
            .args(["analysis", "show", "--session", &session])
            .output()
            .unwrap(),
    );
    assert_eq!(record["materials"].as_object().unwrap().len(), 1);
}

#[test]
fn saved_knowledge_reads_do_not_scan_unrelated_corrupt_runs() {
    let fixture = Fixture::new();
    let session = fixture.session();
    let adopted = success(
        fixture
            .command()
            .args([
                "knowledge",
                "read",
                "--session",
                &session,
                "--pack",
                "review-pack",
                "--entry",
                "code-review",
            ])
            .output()
            .unwrap(),
    );
    let run = fixture
        .home
        .join("sessions")
        .join(&session)
        .join("runs")
        .join(uuid::Uuid::now_v7().to_string());
    fs::create_dir_all(&run).unwrap();
    fs::write(run.join("manifest.json"), b"corrupt").unwrap();
    failure(
        fixture
            .command()
            .args(["inspect", "session", "--session", &session])
            .output()
            .unwrap(),
    );
    for args in [
        vec!["--material", adopted["material_id"].as_str().unwrap()],
        vec!["--pack", "review-pack", "--entry", "code-review"],
    ] {
        let restored = success(
            fixture
                .command()
                .args(["knowledge", "read", "--session", &session])
                .args(args)
                .output()
                .unwrap(),
        );
        assert_eq!(restored["material"], adopted["material"]);
        assert_eq!(restored["revision"], 2);
    }
}

#[test]
fn adoption_requires_goal_and_valid_cli_selection() {
    let fixture = Fixture::new();
    let session = success(
        fixture
            .command()
            .args(["session", "create"])
            .output()
            .unwrap(),
    )["session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let base = ["knowledge", "read", "--session", &session];
    failure(
        fixture
            .command()
            .args(base)
            .args(["--pack", "review-pack", "--entry", "code-review"])
            .output()
            .unwrap(),
    );
    assert!(
        !fixture
            .home
            .join("sessions")
            .join(&session)
            .join("analysis")
            .exists()
    );
    for options in [
        vec!["--pack", "review-pack"],
        vec!["--entry", "code-review"],
        vec![
            "--pack",
            "review-pack",
            "--entry",
            "code-review",
            "--path",
            "knowledge/rules.md",
        ],
        vec!["--material", "id", "--pack", "review-pack"],
        vec!["--material", "id", "--update"],
        vec![
            "--pack",
            "review-pack",
            "--entry",
            "code-review",
            "--update",
        ],
        vec![
            "--pack",
            "review-pack",
            "--entry",
            "code-review",
            "--expected-revision",
            "1",
        ],
    ] {
        let output = fixture.command().args(base).args(options).output().unwrap();
        assert!(!output.status.success());
        assert!(
            output.stdout.is_empty(),
            "argument rejection must not return a result"
        );
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn schema_two_is_read_only_until_successful_write_and_unknown_schemas_fail() {
    let fixture = Fixture::new();
    let session = fixture.session();
    let path = fixture
        .home
        .join("sessions")
        .join(&session)
        .join("analysis/record.json");
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["schema_version"] = json!(2);
    record["report"] = json!({"content":"旧报告"});
    let query_id = "old-query";
    record["materials"][query_id] = json!({"kind":"query","run_id":uuid::Uuid::now_v7().to_string(),"sql":"SELECT 1","outputs":[],"columns":[],"ndjson":"{\"value\":1}\n"});
    let original = serde_json::to_vec(&record).unwrap();
    fs::write(&path, &original).unwrap();
    let shown = success(
        fixture
            .command()
            .args(["analysis", "show", "--session", &session])
            .output()
            .unwrap(),
    );
    assert_eq!(shown["schema_version"], 2);
    assert_eq!(fs::read(&path).unwrap(), original);
    failure(
        fixture
            .command()
            .args([
                "knowledge",
                "read",
                "--session",
                &session,
                "--material",
                query_id,
            ])
            .output()
            .unwrap(),
    );
    success(
        fixture
            .command()
            .args([
                "knowledge",
                "read",
                "--session",
                &session,
                "--pack",
                "review-pack",
                "--entry",
                "code-review",
            ])
            .output()
            .unwrap(),
    );
    let newer: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(newer["schema_version"], 3);
    assert_eq!(newer["materials"][query_id], record["materials"][query_id]);
    assert_eq!(newer["report"], record["report"]);
    for version in [2, 999] {
        let mut corrupt = newer.clone();
        corrupt["schema_version"] = json!(version);
        let bytes = serde_json::to_vec(&corrupt).unwrap();
        fs::write(&path, &bytes).unwrap();
        failure(
            fixture
                .command()
                .args([
                    "knowledge",
                    "read",
                    "--session",
                    &session,
                    "--pack",
                    "review-pack",
                    "--entry",
                    "code-review",
                ])
                .output()
                .unwrap(),
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[cfg(windows)]
#[test]
fn failed_record_replacement_retains_the_previous_record() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let session = fixture.session();
    let path = fixture
        .home
        .join("sessions")
        .join(&session)
        .join("analysis/record.json");
    let original = fs::read(&path).unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    let error = failure(
        fixture
            .command()
            .args([
                "knowledge",
                "read",
                "--session",
                &session,
                "--pack",
                "review-pack",
                "--entry",
                "code-review",
            ])
            .output()
            .unwrap(),
    );
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("cannot replace Analysis Record")
    );
    drop(held);
    assert_eq!(fs::read(path).unwrap(), original);
}

#[cfg(windows)]
#[test]
fn junction_targets_cannot_escape_pack_or_alias_a_declaration() {
    let fixture = Fixture::new();
    let session = fixture.session();
    let outside = fixture.home.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("rules.md"), "outside").unwrap();
    fs::create_dir_all(fixture.pack.join("knowledge")).unwrap();
    for (link, target, path) in [
        ("outside", outside, "knowledge/outside/rules.md"),
        (
            "declarations",
            fixture.pack.join("skills"),
            "knowledge/declarations/code-review/SKILL.md",
        ),
    ] {
        let created = Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(
                fixture
                    .pack
                    .join("knowledge")
                    .join(link)
                    .to_string_lossy()
                    .replace('/', "\\"),
            )
            .arg(target.to_string_lossy().replace('/', "\\"))
            .output()
            .unwrap();
        assert!(
            created.status.success(),
            "{}",
            String::from_utf8_lossy(&created.stderr)
        );
        failure(
            fixture
                .command()
                .args([
                    "knowledge",
                    "read",
                    "--session",
                    &session,
                    "--pack",
                    "review-pack",
                    "--path",
                    path,
                ])
                .output()
                .unwrap(),
        );
    }
    let record = success(
        fixture
            .command()
            .args(["analysis", "show", "--session", &session])
            .output()
            .unwrap(),
    );
    assert_eq!(record["revision"], 1);
    assert_eq!(record["materials"], json!({}));
}

#[test]
#[ignore = "requires KAT_TEST_PYTHON and wheels built from the current checkout"]
fn real_host_combines_knowledge_and_workflow_and_recovers_without_pack() {
    let root = tempfile::tempdir().unwrap();
    let python = PathBuf::from(std::env::var_os("KAT_TEST_PYTHON").unwrap());
    let wheel = PathBuf::from(std::env::var_os("KAT_TEST_WORKFLOW_WHEEL").unwrap());
    let (skill, _) =
        support::stage_real_host_skill(root.path(), &support::cargo_kat(), &python, &wheel);
    let installed = root.path().join("relocated/kat");
    fs::create_dir_all(installed.parent().unwrap()).unwrap();
    fs::rename(skill, &installed).unwrap();
    let binary = if cfg!(windows) {
        installed.join("scripts/targets/windows-x86_64/kat.exe")
    } else {
        installed.join("scripts/targets/linux-x86_64/kat")
    };
    let home = root.path().join("data");
    let pack = home.join("packs/review-pack");
    fs::create_dir_all(pack.join("skills/code-review")).unwrap();
    fs::create_dir_all(pack.join("workflows")).unwrap();
    fs::create_dir_all(pack.join("knowledge")).unwrap();
    fs::create_dir_all(pack.join("knowledge/workflows")).unwrap();
    fs::write(pack.join("pack.toml"), "name = \"review-pack\"\ntitle = \"Review\"\ndescription = \"团队评审经验\"\nowner = \"Review team\"\n").unwrap();
    fs::write(pack.join("skills/code-review/SKILL.md"), ORIGINAL).unwrap();
    fs::write(
        pack.join("knowledge/rules.md"),
        "# 准则\n只有两次测量，不推广到全体用户。\n",
    )
    .unwrap();
    fs::write(
        pack.join("knowledge/workflows/facts.md"),
        "# Facts\n单位毫秒，仅两次测量。\n",
    )
    .unwrap();
    fs::write(
        pack.join("workflows/facts.py"),
        r#"import pyarrow as pa
from kat import Context, workflow, dataprovider as dp

@workflow(name="facts", description="Publish two measurements.", guide="workflows/facts.md")
def facts(ctx: Context):
    return dp.Table.from_arrow(pa.table({"duration_ms": [20, 22]}))
"#,
    )
    .unwrap();
    let fixture = Fixture {
        _root: root,
        binary,
        home,
        pack,
    };
    let session = fixture.session();
    let knowledge = success(
        fixture
            .command()
            .args([
                "knowledge",
                "read",
                "--session",
                &session,
                "--pack",
                "review-pack",
                "--entry",
                "code-review",
            ])
            .output()
            .unwrap(),
    );
    let reference = success(
        fixture
            .command()
            .args([
                "knowledge",
                "read",
                "--session",
                &session,
                "--pack",
                "review-pack",
                "--path",
                "knowledge/rules.md",
            ])
            .output()
            .unwrap(),
    );
    let run = success(
        fixture
            .command()
            .args([
                "run",
                "--session",
                &session,
                "--pack",
                "review-pack",
                "--workflow",
                "facts",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(run["guide"], "# Facts\n单位毫秒，仅两次测量。\n");
    let run_id = run["run_id"].as_str().unwrap();
    let query = success(
        fixture
            .command()
            .args([
                "query",
                "--session",
                &session,
                "--run",
                run_id,
                "--sql",
                "SELECT SUM(duration_ms) AS total_ms FROM output.main",
                "--archive",
            ])
            .output()
            .unwrap(),
    );
    let archive = &query["analysis"];
    assert_eq!(archive["previous_revision"], 3);
    assert_eq!(archive["revision"], 4);
    let report = format!(
        "两次测量合计 42ms，不推广到全体用户。方法 {}、准则 {}，事实 {}。",
        knowledge["material_id"].as_str().unwrap(),
        reference["material_id"].as_str().unwrap(),
        archive["material_id"].as_str().unwrap()
    );
    let input = fixture.home.join("result.json");
    fs::write(
        &input,
        serde_json::to_vec(
            &json!({"nodes":[{"run_id":run_id,"content":report}],"report":{"content":report}}),
        )
        .unwrap(),
    )
    .unwrap();
    success(
        fixture
            .command()
            .args([
                "analysis",
                "save",
                "--session",
                &session,
                "--expected-revision",
                "4",
                "--file",
            ])
            .arg(input)
            .output()
            .unwrap(),
    );
    fs::rename(&fixture.pack, fixture.home.join("removed-pack")).unwrap();
    let recovered = success(
        fixture
            .command()
            .args(["analysis", "show", "--session", &session])
            .output()
            .unwrap(),
    );
    assert_eq!(recovered["report"]["content"], report);
    assert_eq!(recovered["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(recovered["nodes"][0]["run_id"], run_id);
    for receipt in [&knowledge, &reference] {
        let material = success(
            fixture
                .command()
                .args([
                    "knowledge",
                    "read",
                    "--session",
                    &session,
                    "--material",
                    receipt["material_id"].as_str().unwrap(),
                ])
                .output()
                .unwrap(),
        );
        assert_eq!(material["material"], receipt["material"]);
    }
    let evidence = success(
        fixture
            .command()
            .args([
                "analysis",
                "show",
                "--session",
                &session,
                "--material",
                archive["material_id"].as_str().unwrap(),
            ])
            .output()
            .unwrap(),
    );
    assert!(
        evidence["material"]["ndjson"]
            .as_str()
            .unwrap()
            .contains("42")
    );
    assert_eq!(
        success(
            fixture
                .command()
                .args(["inspect", "run", "--session", &session, "--run", run_id])
                .output()
                .unwrap()
        ),
        run
    );
}
