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

    fn update(&self, changes: Vec<Value>) -> Value {
        let revision = self.show()["revision"].as_u64().unwrap();
        let path = self.root.join("changes.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({"changes": changes})).unwrap(),
        )
        .unwrap();
        success(
            self.command()
                .args([
                    "analysis",
                    "update",
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

    fn archive_guide(&self, run: &str) -> Value {
        success(
            self.command()
                .args([
                    "inspect",
                    "workflow",
                    "--session",
                    &self.session,
                    "--run",
                    run,
                    "--archive-to-session",
                    &self.session,
                    "--pack-dir",
                ])
                .arg(&self.pack)
                .output()
                .unwrap(),
        )
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

fn select(run: &str, parent: Option<&str>) -> Value {
    json!({"op": "select_node", "run_id": run, "report_parent": parent, "before": null})
}

fn interpretation(
    run: &str,
    guide: &str,
    evidence: Vec<&str>,
    uses: Vec<Value>,
    conclusion: &str,
) -> Value {
    json!({
        "op": "set_interpretation", "run_id": run,
        "interpretation": {
            "facts": ["测量值为 20 ms 和 22 ms，合计 42 ms"],
            "conclusion": conclusion, "scope": "两次测量",
            "limitations": ["两条数据不足以推断总体趋势"],
            "guide": guide, "evidence": evidence, "uses": uses,
        }
    })
}

fn saved_ref(record: &Value, run: &str) -> Value {
    let node = record["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["run_id"] == run)
        .unwrap();
    assert_eq!(node["interpretation"]["state"], "current");
    json!({"run_id": run, "interpretation_version": node["interpretation"]["interpretation_version"]})
}

#[test]
#[ignore = "requires KAT_TEST_PYTHON and a wheel built from the current checkout"]
fn analysis_restores_a_real_nested_run_tree_and_surviving_child_of_failed_parent() {
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
    let goal_path = cli.root.join("goal.json");
    let goal = json!({"question": "两次测量合计耗时是多少？", "scope": "两次测量", "gaps": []});
    fs::write(&goal_path, serde_json::to_vec(&goal).unwrap()).unwrap();
    success(
        cli.command()
            .args(["analysis", "init", "--session", &cli.session, "--file"])
            .arg(&goal_path)
            .output()
            .unwrap(),
    );

    let parent_guide = success(
        cli.command()
            .args([
                "inspect",
                "workflow",
                "--pack",
                "analysis-real-host",
                "--workflow",
                "parent",
                "--archive-to-session",
                &cli.session,
                "--pack-dir",
            ])
            .arg(&cli.pack)
            .output()
            .unwrap(),
    );
    let parent_guide_id = parent_guide["analysis"]["material_id"].as_str().unwrap();
    let executed = success(cli.run("parent"));
    assert_eq!(
        executed["outputs"],
        json!({}),
        "None parent must not gain output.main"
    );
    let parent_id = executed["run_id"].as_str().unwrap();
    let inventory = cli.call(&["inspect", "session", "--session", &cli.session]);
    let runs = inventory["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 3);
    let subtotal = runs
        .iter()
        .find(|run| run["workflow"] == "subtotal")
        .unwrap();
    let facts = runs.iter().find(|run| run["workflow"] == "facts").unwrap();
    let parent = runs.iter().find(|run| run["run_id"] == parent_id).unwrap();
    let subtotal_id = subtotal["run_id"].as_str().unwrap();
    let facts_id = facts["run_id"].as_str().unwrap();
    assert_eq!(parent["child_runs"], json!([subtotal_id]));
    assert_eq!(subtotal["child_runs"], json!([facts_id]));
    assert_eq!(facts["child_runs"], json!([]));
    assert_eq!(
        cli.show()["nodes"],
        json!([]),
        "ctx.run executes without creating AI conclusions"
    );
    cli.update(vec![
        select(parent_id, None),
        select(subtotal_id, Some(parent_id)),
        select(facts_id, Some(subtotal_id)),
    ]);

    let facts_guide = cli.archive_guide(facts_id);
    let subtotal_guide = cli.archive_guide(subtotal_id);
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
    cli.update(vec![interpretation(
        facts_id,
        facts_guide["analysis"]["material_id"].as_str().unwrap(),
        vec![facts_evidence],
        vec![],
        "两次测量分别耗时20 ms和22 ms",
    )]);

    // 每个 CLI 调用都是新进程；只凭 Session ID 恢复已保存子解释，继续向上汇总。
    let resumed = cli.show();
    assert_eq!(resumed["goal"], goal);
    let facts_ref = saved_ref(&resumed, facts_id);
    let subtotal_saved = cli.update(vec![interpretation(
        subtotal_id,
        subtotal_guide["analysis"]["material_id"].as_str().unwrap(),
        vec![subtotal_evidence],
        vec![facts_ref],
        "两次测量合计42 ms",
    )]);
    let subtotal_ref = saved_ref(&subtotal_saved, subtotal_id);
    let parent_saved = cli.update(vec![interpretation(
        parent_id,
        parent_guide_id,
        vec![],
        vec![subtotal_ref],
        "该组合Workflow的两次测量合计42 ms",
    )]);
    let parent_ref = saved_ref(&parent_saved, parent_id);
    let report = "两次测量合计42 ms；父Workflow没有自身Output。样本量为2，不能推断总体趋势。";
    cli.update(vec![
        json!({"op":"set_report", "report":{"content":report, "uses":[parent_ref]}}),
    ]);
    assert_eq!(cli.show()["report"]["state"], "current");

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
    let surviving_child = after_runs
        .iter()
        .find(|run| run["workflow"] == "facts" && run["run_id"] != facts_id)
        .unwrap();
    let surviving_id = surviving_child["run_id"].as_str().unwrap();
    let surviving_evidence = cli.archive_query(
        surviving_id,
        "SELECT SUM(duration_ms) AS total_ms FROM output.facts",
    );
    assert_eq!(
        cli.material(
            surviving_evidence["analysis"]["material_id"]
                .as_str()
                .unwrap()
        )["run_id"],
        surviving_id
    );
    let selected = cli.update(vec![select(surviving_id, None)]);
    let selected_child = selected["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["run_id"] == surviving_id)
        .unwrap();
    assert!(selected_child["report_parent"].is_null());
    assert!(
        selected_child["selection"].is_null(),
        "unknown AI choices must not be invented"
    );
    assert_eq!(
        selected["report"]["state"], "stale",
        "a changed report tree needs a fresh report"
    );

    fs::rename(&cli.pack, cli.root.join("pack-removed-from-discovery")).unwrap();
    fs::remove_file(facts_query["path"].as_str().unwrap()).unwrap();
    fs::remove_file(subtotal_query["path"].as_str().unwrap()).unwrap();
    let restored = cli.show();
    assert_eq!(restored["report"]["content"], report);
    assert_eq!(restored["nodes"].as_array().unwrap().len(), 4);
    assert_eq!(cli.material(facts_evidence)["ndjson"], original_ndjson);
    assert_eq!(
        cli.material(parent_guide_id)["guide"],
        "# Parent\nUse the subtotal and its child evidence. Parent has no table of its own.\n"
    );
    assert_eq!(saved_ref(&restored, facts_id)["interpretation_version"], 1);
}
