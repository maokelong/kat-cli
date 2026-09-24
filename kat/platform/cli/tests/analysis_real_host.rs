use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::{Value, json};

#[allow(dead_code)]
mod support;

struct AnalysisCli {
    root: PathBuf,
    home: PathBuf,
    binary: PathBuf,
    pack: PathBuf,
    session: String,
}

impl AnalysisCli {
    fn command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        command.env("KAT_DATA_HOME", &self.home);
        command
    }

    fn call(&self, args: &[&str]) -> Value {
        success(self.command().args(args).output().unwrap())
    }

    fn show(&self) -> Value {
        self.call(&["analysis", "show", "--session", &self.session])
    }

    fn save(&self, revision: u64, content: Value) -> Value {
        let path = self.root.join("save.json");
        fs::write(&path, serde_json::to_vec(&content).unwrap()).unwrap();
        success(
            self.command()
                .args([
                    "analysis",
                    "save",
                    "--session",
                    &self.session,
                    "--expected-revision",
                    &revision.to_string(),
                    "--file",
                ])
                .arg(path)
                .output()
                .unwrap(),
        )
    }

    fn run(&self, workflow: &str) -> Output {
        self.command()
            .args([
                "run",
                "--session",
                &self.session,
                "--pack",
                "analysis-real-host",
                "--workflow",
                workflow,
                "--pack-dir",
            ])
            .arg(&self.pack)
            .output()
            .unwrap()
    }

    fn inspect_run(&self, run: &str) -> Value {
        self.call(&["inspect", "run", "--session", &self.session, "--run", run])
    }

    fn archive_query(&self, run: &str, sql: &str) -> Value {
        self.call(&[
            "query",
            "--session",
            &self.session,
            "--run",
            run,
            "--sql",
            sql,
            "--archive",
        ])
    }

    fn material(&self, id: &str) -> Value {
        self.call(&[
            "analysis",
            "show",
            "--session",
            &self.session,
            "--material",
            id,
        ])["material"]
            .clone()
    }
}

fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "success", "{response}");
    response["result"].clone()
}

fn write_pack(pack: &Path) {
    fs::create_dir_all(pack.join("workflows")).unwrap();
    fs::create_dir_all(pack.join("knowledge/workflows")).unwrap();
    fs::write(
        pack.join("pack.toml"),
        "name = \"analysis-real-host\"\ntitle = \"Analysis lifecycle\"\ndescription = \"Real Host fixture\"\nowner = \"Test\"\n",
    )
    .unwrap();
    for (name, code, guide) in [
        (
            "facts",
            r#"import pyarrow as pa
from kat import Context, dataprovider as dp, workflow

@workflow(name="facts", description="Publish two measurements.", guide="workflows/facts.md")
def facts(ctx: Context):
    return {"facts": dp.Table.from_arrow(pa.table({"duration_ms": [20, 22]}))}
"#,
            "# Facts\nTwo measurements in milliseconds; do not infer a population trend.\n",
        ),
        (
            "subtotal",
            r#"from kat import Context, dataprovider as dp, workflow

@workflow(name="subtotal", description="Aggregate one child.", guide="workflows/subtotal.md")
def subtotal(ctx: Context):
    child = ctx.run("analysis-real-host", "facts")
    return dp.DataFusionProvider(catalog=child).query("SELECT SUM(duration_ms) AS total_ms FROM facts")
"#,
            "# Subtotal\nSum the child facts in milliseconds; preserve the two-sample limitation.\n",
        ),
        (
            "parent",
            r#"from kat import Context, workflow

@workflow(name="parent", description="Orchestrate without publishing a table.", guide="workflows/parent.md")
def parent(ctx: Context):
    ctx.run("analysis-real-host", "subtotal")
"#,
            "# Parent\nUse the subtotal and its child evidence. Parent has no table of its own.\n",
        ),
        (
            "mutating-guide",
            r##"from pathlib import Path
from kat import Context, workflow

@workflow(name="mutating-guide", description="Change the Guide during business execution.", guide="workflows/mutating-guide.md")
def mutating_guide(ctx: Context):
    guide = Path(__file__).parents[1] / "knowledge/workflows/mutating-guide.md"
    guide.write_text("# Changed during execution\n", encoding="utf-8", newline="")
"##,
            "# Original method\r\nCaptured before the function starts.\n",
        ),
        (
            "no-guide",
            r#"from kat import Context, workflow

@workflow(name="no-guide", description="Run without a declared Guide.")
def no_guide(ctx: Context):
    return None
"#,
            "",
        ),
        (
            "failed-parent",
            r#"from kat import Context, workflow

@workflow(name="failed-parent", description="Fail after publishing a child.")
def failed_parent(ctx: Context):
    ctx.run("analysis-real-host", "facts")
    raise ValueError("ANALYSIS_PARENT_FAILED_AFTER_CHILD")
"#,
            "",
        ),
    ] {
        fs::write(
            pack.join("workflows")
                .join(format!("{}.py", name.replace('-', "_"))),
            code,
        )
        .unwrap();
        if !guide.is_empty() {
            fs::write(
                pack.join("knowledge/workflows").join(format!("{name}.md")),
                guide,
            )
            .unwrap();
        }
    }
}

fn node<'a>(nodes: &'a Value, run: &str) -> &'a Value {
    fn find<'a>(nodes: &'a Value, run: &str) -> Option<&'a Value> {
        nodes.as_array()?.iter().find_map(|node| {
            if node["run_id"] == run {
                Some(node)
            } else {
                find(&node["children"], run)
            }
        })
    }
    find(nodes, run).unwrap_or_else(|| panic!("missing Run {run} in {nodes}"))
}

#[test]
#[ignore = "requires KAT_TEST_PYTHON and wheels built from the current checkout"]
fn analysis_restores_nested_run_guides_evidence_and_plain_reports_without_pack() {
    let python = PathBuf::from(std::env::var_os("KAT_TEST_PYTHON").unwrap());
    let wheel = PathBuf::from(std::env::var_os("KAT_TEST_WORKFLOW_WHEEL").unwrap());
    let temporary = tempfile::tempdir().unwrap();
    let (_, binary) =
        support::stage_real_host_skill(temporary.path(), &support::cargo_kat(), &python, &wheel);
    let home = temporary.path().join("home");
    fs::create_dir(&home).unwrap();
    let mut cli = AnalysisCli {
        root: temporary.path().to_owned(),
        home,
        binary,
        pack: temporary.path().join("fixture-pack"),
        session: String::new(),
    };
    cli.session = cli.call(&["session", "create"])["session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    write_pack(&cli.pack);

    let executed = success(cli.run("parent"));
    assert_eq!(executed.as_object().unwrap().len(), 5);
    assert_eq!(
        executed["outputs"],
        json!({}),
        "None parent must not gain output.main"
    );
    let parent_id = executed["run_id"].as_str().unwrap();
    let parent_guide =
        "# Parent\nUse the subtotal and its child evidence. Parent has no table of its own.\n";
    assert_eq!(executed["guide"], parent_guide);
    assert_eq!(cli.inspect_run(parent_id), executed);
    assert!(
        !cli.home
            .join("sessions")
            .join(&cli.session)
            .join("analysis")
            .exists()
    );
    let inventory = cli.call(&["inspect", "session", "--session", &cli.session]);
    let runs = inventory["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 3);
    let subtotal = runs
        .iter()
        .find(|run| run["workflow"] == "subtotal")
        .unwrap();
    let facts = runs.iter().find(|run| run["workflow"] == "facts").unwrap();
    let subtotal_id = subtotal["run_id"].as_str().unwrap();
    let facts_id = facts["run_id"].as_str().unwrap();
    assert_eq!(executed["child_runs"], json!([subtotal_id]));
    assert_eq!(subtotal["child_runs"], json!([facts_id]));
    assert_eq!(facts["child_runs"], json!([]));
    let facts_snapshot = cli.inspect_run(facts_id);
    let subtotal_snapshot = cli.inspect_run(subtotal_id);
    assert_eq!(
        facts_snapshot["guide"],
        "# Facts\nTwo measurements in milliseconds; do not infer a population trend.\n"
    );
    assert_eq!(
        subtotal_snapshot["guide"],
        "# Subtotal\nSum the child facts in milliseconds; preserve the two-sample limitation.\n"
    );
    assert_eq!(subtotal_snapshot["child_runs"], json!([facts_id]));

    let goal = json!({"question": "两次测量合计耗时是多少？", "scope": "两次测量", "gaps": []});
    let initialized = cli.save(0, json!({"goal": goal}));
    assert_eq!(initialized["schema_version"], 2);
    assert_eq!(
        initialized["nodes"],
        json!([]),
        "ctx.run does not create AI conclusions"
    );
    let facts_query = cli.archive_query(
        facts_id,
        "SELECT duration_ms FROM output.facts ORDER BY duration_ms",
    );
    let subtotal_query = cli.archive_query(subtotal_id, "SELECT total_ms FROM output.main");
    let facts_evidence = facts_query["analysis"]["material_id"].as_str().unwrap();
    let subtotal_evidence = subtotal_query["analysis"]["material_id"].as_str().unwrap();
    let original_ndjson = fs::read_to_string(facts_query["path"].as_str().unwrap()).unwrap();
    assert_eq!(cli.material(facts_evidence)["ndjson"], original_ndjson);
    let rows: Vec<Value> = original_ndjson
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        rows,
        vec![json!({"duration_ms":20}), json!({"duration_ms":22})]
    );
    let subtotal_rows: Value = serde_json::from_str(
        fs::read_to_string(subtotal_query["path"].as_str().unwrap())
            .unwrap()
            .trim(),
    )
    .unwrap();
    assert_eq!(subtotal_rows, json!({"total_ms":42}));

    let facts_text =
        format!("材料 {facts_evidence}：两次测量分别耗时 20 ms 和 22 ms，不能推断总体趋势。");
    let first = cli.save(
        subtotal_query["analysis"]["revision"].as_u64().unwrap(),
        json!({"nodes":[{"run_id":facts_id,"content":facts_text}]}),
    );
    assert_eq!(
        first["nodes"],
        json!([{"run_id":facts_id,"content":facts_text,"children":[]}])
    );
    let parent_text = "父 Workflow 无自身 Output，需结合子 Run 的两次测量解释。";
    let second = cli.save(
        first["revision"].as_u64().unwrap(),
        json!({"nodes":[{"run_id":parent_id,"content":parent_text}]}),
    );
    assert_eq!(
        second["nodes"].as_array().unwrap().len(),
        2,
        "do not skip an unrecorded immediate parent"
    );
    assert_eq!(node(&second["nodes"], parent_id)["children"], json!([]));

    // 每个 CLI 调用都是新进程；从已存正文恢复，补上中间节点后自动归位。
    let resumed = cli.show();
    assert_eq!(resumed["goal"], goal);
    let subtotal_text =
        format!("材料 {subtotal_evidence}：两次测量合计 42 ms；仅适用于这两个样本。");
    let nested = cli.save(
        resumed["revision"].as_u64().unwrap(),
        json!({"nodes":[{"run_id":subtotal_id,"content":subtotal_text}]}),
    );
    assert_eq!(
        nested["nodes"],
        json!([{
            "run_id":parent_id,"content":parent_text,"children":[{
                "run_id":subtotal_id,"content":subtotal_text,"children":[{
                    "run_id":facts_id,"content":facts_text,"children":[]
                }]
            }]
        }])
    );
    let report = "两次测量合计 42 ms；父 Workflow 没有自身 Output。样本量为 2，不能推断总体趋势。";
    let saved = cli.save(
        nested["revision"].as_u64().unwrap(),
        json!({"report":{"content":report}}),
    );
    assert_eq!(saved["report"], json!({"content":report}));
    let revised = cli.save(saved["revision"].as_u64().unwrap(), json!({"nodes":[{"run_id":facts_id,"content":"两次测量为 20 ms 和 22 ms；下一步需要扩大样本。"}]}));
    assert_eq!(
        revised["report"], saved["report"],
        "editing a node must retain the last saved report"
    );
    assert_eq!(
        node(&revised["nodes"], subtotal_id)["content"],
        subtotal_text
    );

    let failed = cli.run("failed-parent");
    assert!(!failed.status.success());
    let failure: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert!(
        failure["error"]
            .to_string()
            .contains("ANALYSIS_PARENT_FAILED_AFTER_CHILD")
    );
    let after_failure = cli.call(&["inspect", "session", "--session", &cli.session]);
    let after_runs = after_failure["runs"].as_array().unwrap();
    assert_eq!(
        after_runs.len(),
        4,
        "only the completed child survives parent failure"
    );
    assert!(
        !after_runs
            .iter()
            .any(|run| run["workflow"] == "failed-parent")
    );
    let surviving_id = after_runs
        .iter()
        .find(|run| run["workflow"] == "facts" && run["run_id"] != facts_id)
        .unwrap()["run_id"]
        .as_str()
        .unwrap();
    assert_eq!(
        cli.inspect_run(surviving_id)["guide"],
        facts_snapshot["guide"]
    );
    let survivor = cli.save(revised["revision"].as_u64().unwrap(), json!({"nodes":[{"run_id":surviving_id,"content":"父执行失败后保留下来的独立测量；失败父执行没有成功结论。"}]}));
    assert_eq!(survivor["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(
        node(&survivor["nodes"], surviving_id)["children"],
        json!([])
    );
    assert_eq!(survivor["report"], json!({"content":report}));

    let mutated = success(cli.run("mutating-guide"));
    assert_eq!(
        mutated["guide"],
        "# Original method\r\nCaptured before the function starts.\n"
    );
    assert_eq!(
        fs::read_to_string(cli.pack.join("knowledge/workflows/mutating-guide.md")).unwrap(),
        "# Changed during execution\n"
    );
    assert_eq!(
        cli.inspect_run(mutated["run_id"].as_str().unwrap()),
        mutated
    );
    let no_guide = success(cli.run("no-guide"));
    assert_eq!(no_guide.get("guide"), Some(&Value::Null));
    assert_eq!(
        cli.inspect_run(no_guide["run_id"].as_str().unwrap()),
        no_guide
    );

    fs::rename(&cli.pack, cli.root.join("pack-removed-from-discovery")).unwrap();
    fs::remove_file(facts_query["path"].as_str().unwrap()).unwrap();
    fs::remove_file(subtotal_query["path"].as_str().unwrap()).unwrap();
    let restored = cli.show();
    assert_eq!(restored, survivor);
    assert_eq!(cli.material(facts_evidence)["ndjson"], original_ndjson);
    assert_eq!(cli.inspect_run(parent_id), executed);
    assert_eq!(cli.inspect_run(facts_id), facts_snapshot);
    assert_eq!(cli.inspect_run(subtotal_id), subtotal_snapshot);
    assert_eq!(
        cli.inspect_run(mutated["run_id"].as_str().unwrap()),
        mutated
    );
}
