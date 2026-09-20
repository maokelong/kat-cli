use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

fn command(home: &Path) -> Command {
    fs::create_dir_all(home).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_kat"));
    command.env("KAT_DATA_HOME", home);
    command
}

fn success(output: std::process::Output) -> Value {
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

fn failure(output: Output) {
    assert!(
        !output.status.success(),
        "unexpected success: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "expected a structured failure: {error}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(response["status"], "failure");
    assert!(
        response["error"]["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty())
    );
}

fn wait_bounded(mut child: Child) -> Output {
    let stdout = child.stdout.take().map(|mut stream| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).unwrap();
            bytes
        })
    });
    let stderr = child.stderr.take().map(|mut stream| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).unwrap();
            bytes
        })
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("analysis child exceeded 20 seconds");
        }
        thread::sleep(Duration::from_millis(10));
    };
    Output {
        status,
        stdout: stdout
            .map(|reader| reader.join().unwrap())
            .unwrap_or_default(),
        stderr: stderr
            .map(|reader| reader.join().unwrap())
            .unwrap_or_default(),
    }
}

const RUN_A: &str = "019f6e00-0000-7000-8000-000000000141";
const RUN_B: &str = "019f6e00-0000-7000-8000-000000000142";
const RUN_C: &str = "019f6e00-0000-7000-8000-000000000143";
const RUN_D: &str = "019f6e00-0000-7000-8000-000000000144";
const MISSING_RUN: &str = "019f6e00-0000-7000-8000-000000000145";

struct Fixture {
    temporary: tempfile::TempDir,
    home: PathBuf,
    session_id: String,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path().join("home");
        let session = success(command(&home).args(["session", "create"]).output().unwrap());
        let fixture = Self {
            temporary,
            home,
            session_id: session["session_id"].as_str().unwrap().to_owned(),
        };
        success(fixture.init());
        fixture
    }

    fn goal() -> Value {
        json!({"question": "启动为什么变慢？", "scope": "一次启动", "gaps": []})
    }

    fn input(&self, filename: &str, value: Value) -> PathBuf {
        let path = self.temporary.path().join(filename);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        path
    }

    fn init(&self) -> Output {
        command(&self.home)
            .args(["analysis", "init", "--session", &self.session_id, "--file"])
            .arg(self.input("goal.json", Self::goal()))
            .output()
            .unwrap()
    }

    fn show(&self) -> Value {
        success(
            command(&self.home)
                .args(["analysis", "show", "--session", &self.session_id])
                .output()
                .unwrap(),
        )
    }

    fn show_run(&self, run: &str) -> Value {
        success(
            command(&self.home)
                .args([
                    "analysis",
                    "show",
                    "--session",
                    &self.session_id,
                    "--run",
                    run,
                ])
                .output()
                .unwrap(),
        )["node"]
            .clone()
    }

    fn update_output(&self, revision: u64, changes: Vec<Value>) -> Output {
        command(&self.home)
            .args([
                "analysis",
                "update",
                "--session",
                &self.session_id,
                "--expected-revision",
                &revision.to_string(),
                "--file",
            ])
            .arg(self.input("changes.json", json!({"changes": changes})))
            .output()
            .unwrap()
    }

    fn update(&self, changes: Vec<Value>) -> Value {
        success(self.update_output(self.show()["revision"].as_u64().unwrap(), changes))
    }

    fn session_path(&self) -> PathBuf {
        self.home.join("sessions").join(&self.session_id)
    }

    fn record_path(&self) -> PathBuf {
        self.session_path().join("analysis").join("record.json")
    }

    fn publish_run(&self, run: &str, children: &[&str]) {
        let path = self.session_path().join("runs").join(run);
        fs::create_dir_all(path.join("outputs")).unwrap();
        fs::write(
            path.join("manifest.json"),
            serde_json::to_vec(&json!({
                "session_id": self.session_id,
                "run_id": run,
                "pack": "analysis-pack",
                "workflow": "comparison",
                "child_runs": children,
                "inputs": {},
                "outputs": {}
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn publish_tree(&self) {
        self.publish_run(RUN_A, &[RUN_B, RUN_C]);
        self.publish_run(RUN_B, &[]);
        self.publish_run(RUN_C, &[]);
        self.publish_run(RUN_D, &[]);
    }

    fn prepare_interpretations(&self) {
        self.publish_tree();
        self.update(vec![
            select(RUN_A, None, None),
            select(RUN_B, Some(RUN_A), None),
            select(RUN_C, Some(RUN_A), None),
            select(RUN_D, None, None),
        ]);
        // 这里模拟恢复已归档的记录；实际材料采集由独立的 CLI archive 集成用例覆盖。
        let mut record: Value =
            serde_json::from_slice(&fs::read(self.record_path()).unwrap()).unwrap();
        record["materials"]["019f6e00-0000-7000-8000-000000000150"] = json!({
            "kind": "guide",
            "pack": "analysis-pack",
            "workflow": "comparison",
            "guide": "按相同单位比较，明确缺失的证据。"
        });
        fs::write(self.record_path(), serde_json::to_vec(&record).unwrap()).unwrap();
    }
}

fn select(run: &str, parent: Option<&str>, before: Option<&str>) -> Value {
    json!({"op": "select_node", "run_id": run, "report_parent": parent, "before": before})
}

fn set_goal(question: &str) -> Value {
    json!({"op": "set_goal", "goal": {"question": question, "scope": "一次启动", "gaps": []}})
}

fn interpret(run: &str, conclusion: &str, uses: &[(&str, u64)]) -> Value {
    json!({
        "op": "set_interpretation",
        "run_id": run,
        "interpretation": {
            "facts": ["等待占比为 60%"],
            "conclusion": conclusion,
            "scope": "当前采样窗口",
            "limitations": ["未覆盖其他启动"],
            "guide": "019f6e00-0000-7000-8000-000000000150",
            "evidence": [],
            "uses": uses.iter().map(|(run_id, version)| json!({
                "run_id": run_id,
                "interpretation_version": version
            })).collect::<Vec<_>>()
        }
    })
}

fn report(uses: &[(&str, u64)]) -> Value {
    json!({
        "op": "set_report",
        "report": {
            "content": "启动耗时增加主要来自等待；适用范围是当前窗口。",
            "uses": uses.iter().map(|(run_id, version)| json!({
                "run_id": run_id,
                "interpretation_version": version
            })).collect::<Vec<_>>()
        }
    })
}

fn run_order(record: &Value, parent: Option<&str>) -> Vec<String> {
    record["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|node| node["report_parent"].as_str() == parent)
        .map(|node| node["run_id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn analysis_init_and_show_restore_the_goal() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().join("home");
    let session = success(command(&home).args(["session", "create"]).output().unwrap());
    let session_id = session["session_id"].as_str().unwrap();
    let goal = json!({
        "question": "为什么启动耗时增加？",
        "scope": "比较同一设备的两次启动",
        "gaps": ["尚未确认调度等待"]
    });
    let input = temporary.path().join("goal.json");
    fs::write(&input, serde_json::to_vec(&goal).unwrap()).unwrap();

    let initialized = success(
        command(&home)
            .args(["analysis", "init", "--session", session_id, "--file"])
            .arg(&input)
            .output()
            .unwrap(),
    );
    assert_eq!(initialized["session_id"], session_id);
    assert_eq!(initialized["revision"], 1);

    let restored = success(
        command(&home)
            .args(["analysis", "show", "--session", session_id])
            .output()
            .unwrap(),
    );
    assert_eq!(restored["goal"], goal);
    assert_eq!(restored["revision"], 1);
    assert_eq!(restored["nodes"], json!([]));
    assert!(restored["report"].is_null());
}

#[test]
fn analysis_init_refuses_to_overwrite_an_existing_record() {
    let fixture = Fixture::new();
    let original = fs::read(fixture.record_path()).unwrap();
    failure(fixture.init());
    assert_eq!(fs::read(fixture.record_path()).unwrap(), original);
    assert_eq!(fixture.show()["revision"], 1);
}

#[test]
fn analysis_selects_and_reparents_one_node_per_run_preserving_ai_selection() {
    let fixture = Fixture::new();
    fixture.publish_tree();
    fixture.update(vec![select(RUN_B, None, None), select(RUN_D, None, None)]);
    let selection = json!({
        "source_runs": [RUN_D],
        "materials": [],
        "finding": "已有数据表明等待占比增加",
        "reason": "继续检查等待来源",
        "input_sources": {"duration": "来自 RUN_D 的观测"}
    });
    fixture.update(vec![
        json!({"op": "set_selection", "run_id": RUN_B, "selection": selection}),
    ]);

    let restored = fixture.show_run(RUN_B);
    assert!(restored["report_parent"].is_null());
    assert_eq!(restored["selection"], selection);
    assert!(restored["interpretation"].is_null());

    let record = fixture.update(vec![
        select(RUN_A, None, Some(RUN_D)),
        select(RUN_B, Some(RUN_A), None),
        select(RUN_C, Some(RUN_A), Some(RUN_B)),
    ]);
    assert_eq!(record["nodes"].as_array().unwrap().len(), 4);
    assert_eq!(run_order(&record, None), [RUN_A, RUN_D]);
    assert_eq!(run_order(&record, Some(RUN_A)), [RUN_C, RUN_B]);
    assert_eq!(fixture.show_run(RUN_B)["selection"], selection);
    assert_eq!(fixture.show_run(RUN_B)["report_parent"], RUN_A);

    let reordered = fixture.update(vec![select(RUN_B, Some(RUN_A), Some(RUN_C))]);
    assert_eq!(run_order(&reordered, Some(RUN_A)), [RUN_B, RUN_C]);
    assert_eq!(reordered["nodes"].as_array().unwrap().len(), 4);
    assert!(fixture.show_run(RUN_A)["selection"].is_null());
}

#[test]
fn analysis_rejects_invented_edges_and_invalid_batch_without_partial_changes() {
    let fixture = Fixture::new();
    fixture.publish_tree();
    fixture.update(vec![
        select(RUN_A, None, None),
        select(RUN_B, Some(RUN_A), None),
    ]);
    for invalid in [
        select(RUN_D, Some(RUN_A), None),
        select(RUN_A, Some(RUN_B), None),
        select(RUN_C, Some(RUN_D), None),
        select(MISSING_RUN, None, None),
        select(RUN_C, Some(RUN_A), Some(RUN_A)),
    ] {
        let original = fs::read(fixture.record_path()).unwrap();
        let revision = fixture.show()["revision"].as_u64().unwrap();
        failure(fixture.update_output(revision, vec![set_goal("这项改动也必须回滚"), invalid]));
        assert_eq!(fs::read(fixture.record_path()).unwrap(), original);
    }
}

#[test]
fn analysis_compare_and_swap_rejects_old_revision_without_overwrite() {
    let fixture = Fixture::new();
    let saved = success(fixture.update_output(1, vec![set_goal("已更新的目标")]));
    assert_eq!(saved["revision"], 2);
    let original = fs::read(fixture.record_path()).unwrap();
    failure(fixture.update_output(1, vec![set_goal("过期任务的目标")]));
    assert_eq!(fs::read(fixture.record_path()).unwrap(), original);
    assert_eq!(fixture.show()["goal"]["question"], "已更新的目标");
}

#[test]
fn analysis_concurrent_writers_publish_exactly_one_revision_winner() {
    let fixture = Fixture::new();
    let mut children = Vec::new();
    for index in 0..6 {
        let path = fixture.input(
            &format!("writer-{index}.json"),
            json!({"changes": [set_goal(&format!("writer-{index}"))]}),
        );
        children.push((
            index,
            command(&fixture.home)
                .args([
                    "analysis",
                    "update",
                    "--session",
                    &fixture.session_id,
                    "--expected-revision",
                    "1",
                    "--file",
                ])
                .arg(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ));
    }
    let winners = children
        .into_iter()
        .filter_map(|(index, child)| wait_bounded(child).status.success().then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(
        winners.len(),
        1,
        "writers that reported success: {winners:?}"
    );
    let restored = fixture.show();
    assert_eq!(restored["revision"], 2);
    assert_eq!(
        restored["goal"]["question"],
        format!("writer-{}", winners[0])
    );
}

#[test]
fn analysis_corrupt_and_unsupported_records_are_never_silently_recreated() {
    let fixture = Fixture::new();
    let original: Value =
        serde_json::from_slice(&fs::read(fixture.record_path()).unwrap()).unwrap();
    let mut unsupported = original;
    unsupported["schema_version"] = json!(u32::MAX);
    for bytes in [
        b"{corrupt".to_vec(),
        serde_json::to_vec(&unsupported).unwrap(),
    ] {
        fs::write(fixture.record_path(), &bytes).unwrap();
        failure(
            command(&fixture.home)
                .args(["analysis", "show", "--session", &fixture.session_id])
                .output()
                .unwrap(),
        );
        failure(fixture.init());
        failure(fixture.update_output(1, vec![set_goal("不应覆盖损坏记录")]));
        assert_eq!(fs::read(fixture.record_path()).unwrap(), bytes);
    }
}

#[test]
fn deleting_session_also_removes_the_analysis_record() {
    let fixture = Fixture::new();
    assert!(fixture.record_path().is_file());
    success(
        command(&fixture.home)
            .args(["session", "delete", "--session", &fixture.session_id])
            .output()
            .unwrap(),
    );
    assert!(!fixture.session_path().exists());
    failure(
        command(&fixture.home)
            .args(["analysis", "show", "--session", &fixture.session_id])
            .output()
            .unwrap(),
    );
}

#[test]
fn interpretation_updates_invalidate_actual_consumers_across_report_branches() {
    let fixture = Fixture::new();
    fixture.prepare_interpretations();
    fixture.update(vec![interpret(RUN_B, "子节点 B 的初次结论", &[])]);
    fixture.update(vec![interpret(RUN_C, "C 引用 B", &[(RUN_B, 1)])]);
    fixture.update(vec![interpret(RUN_D, "另一分支 D 引用 C", &[(RUN_C, 1)])]);
    fixture.update(vec![interpret(RUN_A, "父节点 A 引用 D", &[(RUN_D, 1)])]);
    fixture.update(vec![report(&[(RUN_A, 1)])]);
    assert_eq!(fixture.show()["report"]["state"], "current");

    fixture.update(vec![interpret(RUN_B, "补充证据后的 B 结论", &[])]);
    assert_eq!(
        fixture.show_run(RUN_B)["interpretation"]["state"],
        "current"
    );
    assert_eq!(
        fixture.show_run(RUN_B)["interpretation"]["interpretation_version"],
        2
    );
    for run in [RUN_C, RUN_D, RUN_A] {
        let node = fixture.show_run(run);
        assert_eq!(node["interpretation"]["state"], "stale", "{run}");
        assert_eq!(node["interpretation"]["interpretation_version"], 1);
    }
    assert_eq!(fixture.show()["report"]["state"], "stale");
    assert_eq!(
        fixture.show()["report"]["content"],
        report(&[])["report"]["content"]
    );

    fixture.update(vec![interpret(RUN_C, "C 已采用新的 B", &[(RUN_B, 2)])]);
    fixture.update(vec![interpret(RUN_D, "D 已采用新的 C", &[(RUN_C, 2)])]);
    fixture.update(vec![interpret(RUN_A, "A 已采用新的 D", &[(RUN_D, 2)])]);
    fixture.update(vec![report(&[(RUN_A, 2)])]);
    assert_eq!(fixture.show()["report"]["state"], "current");
}

#[test]
fn ai_selection_sources_do_not_become_interpretation_dependencies() {
    let fixture = Fixture::new();
    fixture.prepare_interpretations();
    fixture.update(vec![interpret(RUN_B, "B 的结论", &[])]);
    fixture.update(vec![json!({
        "op": "set_selection",
        "run_id": RUN_D,
        "selection": {
            "source_runs": [RUN_B],
            "materials": ["019f6e00-0000-7000-8000-000000000150"],
            "finding": "B 提示需要进一步检查",
            "reason": "补充独立证据",
            "input_sources": {}
        }
    })]);
    fixture.update(vec![interpret(RUN_D, "D 仅依据自己的证据", &[])]);
    fixture.update(vec![report(&[(RUN_D, 1)])]);

    fixture.update(vec![interpret(RUN_B, "B 后续修正", &[])]);
    assert_eq!(
        fixture.show_run(RUN_D)["interpretation"]["state"],
        "current"
    );
    assert_eq!(fixture.show()["report"]["state"], "current");
    assert_eq!(
        fixture.show_run(RUN_D)["selection"]["source_runs"],
        json!([RUN_B])
    );
}

#[test]
fn report_reordering_preserves_node_interpretations_and_invalidates_only_report() {
    let fixture = Fixture::new();
    fixture.prepare_interpretations();
    fixture.update(vec![interpret(RUN_B, "B 结论", &[])]);
    fixture.update(vec![interpret(RUN_C, "C 结论", &[])]);
    fixture.update(vec![report(&[(RUN_B, 1), (RUN_C, 1)])]);
    let b = fixture.show_run(RUN_B)["interpretation"].clone();
    let c = fixture.show_run(RUN_C)["interpretation"].clone();

    fixture.update(vec![select(RUN_C, Some(RUN_A), Some(RUN_B))]);
    assert_eq!(fixture.show_run(RUN_B)["interpretation"], b);
    assert_eq!(fixture.show_run(RUN_C)["interpretation"], c);
    assert_eq!(fixture.show()["report"]["state"], "stale");
    assert_eq!(run_order(&fixture.show(), Some(RUN_A)), [RUN_C, RUN_B]);
}

#[test]
fn changing_analysis_goal_invalidates_existing_interpretations_and_report() {
    let fixture = Fixture::new();
    fixture.prepare_interpretations();
    fixture.update(vec![interpret(RUN_B, "B 结论", &[])]);
    fixture.update(vec![report(&[(RUN_B, 1)])]);
    fixture.update(vec![set_goal("重新分析不同目标")]);
    assert_eq!(fixture.show_run(RUN_B)["interpretation"]["state"], "stale");
    assert_eq!(fixture.show()["report"]["state"], "stale");
}

#[test]
fn analysis_consumers_require_already_saved_current_interpretation_versions() {
    let fixture = Fixture::new();
    fixture.prepare_interpretations();
    fixture.update(vec![interpret(RUN_B, "B 初始结论", &[])]);
    fixture.update(vec![interpret(RUN_C, "C 引用 B", &[(RUN_B, 1)])]);
    for changes in [
        vec![interpret(RUN_D, "不能引用不存在的版本", &[(RUN_B, 2)])],
        vec![interpret(RUN_B, "不能引用自己", &[(RUN_B, 1)])],
        vec![interpret(RUN_B, "会造成循环的引用", &[(RUN_C, 1)])],
        vec![
            interpret(RUN_B, "更新 B", &[]),
            interpret(RUN_D, "同批旧版本", &[(RUN_B, 1)]),
        ],
        vec![
            interpret(RUN_B, "更新 B", &[]),
            interpret(RUN_D, "同批猜测新版本", &[(RUN_B, 2)]),
        ],
        vec![interpret(RUN_B, "更新 B", &[]), report(&[(RUN_B, 2)])],
        vec![
            interpret(RUN_B, "更新 B", &[]),
            interpret(RUN_D, "C 会间接失效", &[(RUN_C, 1)]),
        ],
        vec![
            interpret(RUN_D, "顺序不能绕过间接依赖校验", &[(RUN_C, 1)]),
            interpret(RUN_B, "后更新 B", &[]),
        ],
        vec![report(&[(RUN_C, 1)]), interpret(RUN_B, "后更新 B", &[])],
    ] {
        let original = fs::read(fixture.record_path()).unwrap();
        let revision = fixture.show()["revision"].as_u64().unwrap();
        failure(fixture.update_output(revision, changes));
        assert_eq!(fs::read(fixture.record_path()).unwrap(), original);
    }

    fixture.update(vec![interpret(RUN_B, "更新后的 B", &[])]);
    let revision = fixture.show()["revision"].as_u64().unwrap();
    failure(fixture.update_output(
        revision,
        vec![interpret(RUN_D, "不能采用 stale C", &[(RUN_C, 1)])],
    ));
    failure(fixture.update_output(revision, vec![report(&[(RUN_C, 1)])]));
    assert_eq!(fixture.show()["revision"], revision);
}

#[test]
fn analysis_update_rejects_forged_materials_versions_and_states() {
    let fixture = Fixture::new();
    fixture.prepare_interpretations();
    let mut forged_version = interpret(RUN_B, "越过 CLI 版本管理", &[]);
    forged_version["interpretation"]["interpretation_version"] = json!(99);
    let mut forged_state = interpret(RUN_B, "越过 CLI 状态管理", &[]);
    forged_state["interpretation"]["state"] = json!("current");
    let mut wrong_guide = interpret(RUN_B, "未归档的 Guide", &[]);
    wrong_guide["interpretation"]["guide"] = json!("missing-guide");
    for change in [
        forged_version,
        forged_state,
        wrong_guide,
        json!({"op": "set_material", "material_id": "made-up", "material": {"kind": "guide", "guide": "伪造文本"}}),
    ] {
        let original = fs::read(fixture.record_path()).unwrap();
        let revision = fixture.show()["revision"].as_u64().unwrap();
        failure(fixture.update_output(revision, vec![change]));
        assert_eq!(fs::read(fixture.record_path()).unwrap(), original);
    }
}

#[test]
fn analysis_reuses_query_evidence_across_branches_without_copying_or_reparenting_nodes() {
    let fixture = Fixture::new();
    fixture.prepare_interpretations();
    const QUERY: &str = "019f6e00-0000-7000-8000-000000000151";
    let raw_evidence = "{\"duration\":18446744073709551615}\n";
    let mut stored: Value =
        serde_json::from_slice(&fs::read(fixture.record_path()).unwrap()).unwrap();
    stored["materials"][QUERY] = json!({
        "kind": "query",
        "run_id": RUN_D,
        "sql": "SELECT duration FROM output.main",
        "outputs": ["main"],
        "columns": [{"name": "duration", "type": "uint64"}],
        "ndjson": raw_evidence
    });
    fs::write(fixture.record_path(), serde_json::to_vec(&stored).unwrap()).unwrap();

    let mut conclusion = interpret(RUN_B, "采用另一分支 D 的证据", &[]);
    conclusion["interpretation"]["evidence"] = json!([QUERY]);
    let saved = fixture.update(vec![conclusion]);
    assert_eq!(saved["nodes"].as_array().unwrap().len(), 4);
    assert_eq!(fixture.show_run(RUN_B)["report_parent"], RUN_A);
    assert!(fixture.show_run(RUN_D)["report_parent"].is_null());
    assert_eq!(
        fixture.show_run(RUN_B)["interpretation"]["evidence"],
        json!([QUERY])
    );
    let material = success(
        command(&fixture.home)
            .args([
                "analysis",
                "show",
                "--session",
                &fixture.session_id,
                "--material",
                QUERY,
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(material["material"]["run_id"], RUN_D);
    assert_eq!(material["material"]["ndjson"], raw_evidence);
}

#[test]
fn analysis_concurrent_readers_observe_only_complete_committed_records() {
    let fixture = Fixture::new();
    let start = std::sync::Barrier::new(2);
    thread::scope(|scope| {
        scope.spawn(|| {
            start.wait();
            for previous_revision in 1..=12 {
                let result = success(fixture.update_output(
                    previous_revision,
                    vec![set_goal(&format!("revision-{}", previous_revision + 1))],
                ));
                assert_eq!(result["revision"], previous_revision + 1);
            }
        });
        start.wait();
        for _ in 0..30 {
            let restored = fixture.show();
            let revision = restored["revision"].as_u64().unwrap();
            if revision == 1 {
                assert_eq!(restored["goal"], Fixture::goal());
            } else {
                assert_eq!(restored["goal"]["question"], format!("revision-{revision}"));
            }
            assert_eq!(restored["session_id"], fixture.session_id);
        }
    });
    assert_eq!(fixture.show()["revision"], 13);
}

#[cfg(windows)]
#[test]
fn failed_atomic_replacement_preserves_previous_record_on_windows() {
    use std::os::windows::fs::OpenOptionsExt;

    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

    let fixture = Fixture::new();
    let original = fs::read(fixture.record_path()).unwrap();
    // 允许正常读取，拒绝替换所需的 delete sharing，制造真实的提交阶段故障。
    let held_record = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(fixture.record_path())
        .unwrap();
    failure(fixture.update_output(1, vec![set_goal("不能提交的目标")]));
    drop(held_record);

    assert_eq!(fs::read(fixture.record_path()).unwrap(), original);
    assert_eq!(fixture.show()["revision"], 1);
    assert_eq!(fixture.show()["goal"], Fixture::goal());
    assert_eq!(
        fixture.update(vec![set_goal("恢复后可以提交")])["revision"],
        2
    );
}

#[test]
fn committed_analysis_survives_failure_to_publish_response_to_closed_stdout() {
    let fixture = Fixture::new();
    let question = "完整提交后的结论".repeat(32 * 1024);
    let input = fixture.input(
        "closed-pipe.json",
        json!({"changes": [set_goal(&question)]}),
    );
    let mut child = command(&fixture.home)
        .args([
            "analysis",
            "update",
            "--session",
            &fixture.session_id,
            "--expected-revision",
            "1",
            "--file",
        ])
        .arg(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = wait_bounded(child);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("write KAT Response"));

    let restored = fixture.show();
    assert_eq!(restored["revision"], 2);
    assert_eq!(restored["goal"]["question"], question);
}
