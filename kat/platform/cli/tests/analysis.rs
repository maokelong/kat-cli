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

fn failure(output: Output) -> String {
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
    let message = response["error"]["message"].as_str().unwrap();
    assert!(!message.is_empty());
    message.to_owned()
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
    fn empty() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path().join("home");
        let session = success(command(&home).args(["session", "create"]).output().unwrap());
        Self {
            temporary,
            home,
            session_id: session["session_id"].as_str().unwrap().to_owned(),
        }
    }

    fn new() -> Self {
        let fixture = Self::empty();
        success(fixture.save_output(0, json!({"goal": Self::goal()})));
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

    fn show_output(&self) -> Output {
        command(&self.home)
            .args(["analysis", "show", "--session", &self.session_id])
            .output()
            .unwrap()
    }

    fn show(&self) -> Value {
        success(self.show_output())
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

    fn save_command(&self, revision: u64, input: &Path) -> Command {
        let mut result = command(&self.home);
        result
            .args([
                "analysis",
                "save",
                "--session",
                &self.session_id,
                "--expected-revision",
                &revision.to_string(),
                "--file",
            ])
            .arg(input);
        result
    }

    fn save_output(&self, revision: u64, value: Value) -> Output {
        self.save_command(revision, &self.input("save.json", value))
            .output()
            .unwrap()
    }

    fn save(&self, value: Value) -> Value {
        success(self.save_output(self.show()["revision"].as_u64().unwrap(), value))
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
                "guide": null,
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
}

fn node(run: &str, content: &str) -> Value {
    json!({"run_id": run, "content": content})
}

fn goal(question: &str) -> Value {
    json!({"question": question, "scope": "一次启动", "gaps": []})
}

fn roots(record: &Value) -> Vec<&str> {
    record["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["run_id"].as_str().unwrap())
        .collect()
}

#[test]
fn analysis_save_creates_plain_content_record() {
    let fixture = Fixture::empty();
    let result = success(fixture.save_output(
        0,
        json!({
            "goal": Fixture::goal(),
            "report": {"content": "现有证据不足，待查询输出。"}
        }),
    ));
    assert_eq!(result["schema_version"], 2);
    assert_eq!(result["revision"], 1);
    assert_eq!(result["goal"], Fixture::goal());
    assert_eq!(
        result["report"],
        json!({"content": "现有证据不足，待查询输出。"})
    );
    assert_eq!(result["nodes"], json!([]));
    assert_eq!(fixture.show(), result);
}

#[test]
fn first_save_requires_goal_and_zero_revision_without_overwriting_existing_record() {
    let fixture = Fixture::empty();
    assert!(failure(fixture.show_output()).contains("does not exist"));
    assert!(
        failure(fixture.save_output(1, json!({"goal": Fixture::goal()})))
            .contains("does not exist")
    );
    assert!(
        failure(fixture.save_output(0, json!({"report": {"content": "no goal"}})))
            .contains("must include")
    );
    assert!(!fixture.record_path().exists());
    success(fixture.save_output(0, json!({"goal": Fixture::goal()})));
    let original = fs::read(fixture.record_path()).unwrap();
    assert!(
        failure(fixture.save_output(0, json!({"goal": goal("不能覆盖")})))
            .contains("revision conflict")
    );
    assert_eq!(fs::read(fixture.record_path()).unwrap(), original);
}

#[test]
fn independent_runs_are_root_siblings_and_direct_parent_is_derived_when_saved() {
    let fixture = Fixture::new();
    fixture.publish_tree();
    let selected = fixture
        .save(json!({"nodes": [node(RUN_B, "B 的解释"), node(RUN_D, "根据 B 的发现继续查询 D")]}));
    assert_eq!(roots(&selected), vec![RUN_B, RUN_D]);
    let result = fixture.save(json!({"nodes": [node(RUN_A, "父级解释")]}));
    assert_eq!(roots(&result), vec![RUN_D, RUN_A]);
    assert_eq!(
        result["nodes"][1]["children"],
        json!([{ "run_id": RUN_B, "content": "B 的解释", "children": [] }])
    );
    assert_eq!(fixture.show_run(RUN_B), node(RUN_B, "B 的解释"));
    let persisted: Value =
        serde_json::from_slice(&fs::read(fixture.record_path()).unwrap()).unwrap();
    assert_eq!(
        persisted["nodes"],
        json!([
            node(RUN_B, "B 的解释"),
            node(RUN_D, "根据 B 的发现继续查询 D"),
            node(RUN_A, "父级解释")
        ])
    );
    assert!(persisted.get("tree").is_none());
}

#[test]
fn tree_never_skips_an_unselected_intermediate_parent() {
    let fixture = Fixture::new();
    fixture.publish_run(RUN_A, &[RUN_B]);
    fixture.publish_run(RUN_B, &[RUN_C]);
    fixture.publish_run(RUN_C, &[]);
    let result = fixture.save(json!({"nodes": [node(RUN_A, "A"), node(RUN_C, "C")]}));
    assert_eq!(roots(&result), vec![RUN_A, RUN_C]);
    assert_eq!(result["nodes"][0]["children"], json!([]));
    let result = fixture.save(json!({"nodes": [node(RUN_B, "B")]}));
    assert_eq!(roots(&result), vec![RUN_A]);
    assert_eq!(result["nodes"][0]["children"][0]["run_id"], RUN_B);
    assert_eq!(
        result["nodes"][0]["children"][0]["children"][0]["run_id"],
        RUN_C
    );
}

#[test]
fn partial_save_preserves_other_content_and_report_without_validity_fields() {
    let fixture = Fixture::new();
    fixture.publish_tree();
    fixture.save(json!({"nodes": [node(RUN_B, "B 原文"), node(RUN_D, "D 原文")], "report": {"content": "旧总报告"}}));
    let result = fixture.save(json!({"goal": goal("更新问题"), "nodes": [node(RUN_B, "B 新正文"), node(RUN_A, "A 汇总")]}));
    assert_eq!(result["goal"]["question"], "更新问题");
    assert_eq!(result["report"], json!({"content": "旧总报告"}));
    assert_eq!(fixture.show_run(RUN_B), node(RUN_B, "B 新正文"));
    assert_eq!(fixture.show_run(RUN_D), node(RUN_D, "D 原文"));
    let persisted: Value =
        serde_json::from_slice(&fs::read(fixture.record_path()).unwrap()).unwrap();
    for saved in persisted["nodes"].as_array().unwrap() {
        assert_eq!(saved.as_object().unwrap().len(), 2);
    }
    let report_only = fixture.save(json!({"report": {"content": "新的总报告"}}));
    assert_eq!(report_only["nodes"], result["nodes"]);
    assert_eq!(report_only["goal"], result["goal"]);
}

#[test]
fn invalid_save_is_rejected_without_any_partial_change() {
    let fixture = Fixture::new();
    fixture.publish_tree();
    fixture.save(json!({"nodes": [node(RUN_A, "原解释")]}));
    let original = fs::read(fixture.record_path()).unwrap();
    let invalid_inputs = [
        json!({}),
        json!({"nodes": []}),
        json!({"nodes": null}),
        json!({"goal": null}),
        json!({"report": null}),
        json!({"nodes": [node(RUN_A, "新解释"), node(RUN_A, "重复")]}),
        json!({"nodes": [node(RUN_A, "新解释"), node(MISSING_RUN, "没有发布")]}),
        json!({"nodes": [node("invalid", "非法身份")]}),
        json!({"nodes": [node(RUN_B, " \n")]}),
        json!({"goal": goal(" ")}),
        json!({"report": {"content": " "}}),
        json!({"nodes": [{"run_id": RUN_A, "content": "x", "uses": []}]}),
        json!({"nodes": [{"run_id": RUN_A, "content": "x", "report_parent": null}]}),
        json!({"report": {"content": "x", "state": "current"}}),
        json!({"materials": {}}),
        json!({"changes": []}),
    ];
    for input in invalid_inputs {
        failure(fixture.save_output(2, input));
        assert_eq!(fs::read(fixture.record_path()).unwrap(), original);
    }
}

#[test]
fn saved_node_identity_must_belong_to_its_own_session() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    other.publish_run(RUN_A, &[]);
    failure(fixture.save_output(1, json!({"nodes": [node(RUN_A, "别的 Session")]})));
    assert_eq!(fixture.show()["nodes"], json!([]));
}

#[test]
fn cyclic_or_ambiguous_published_parent_relations_cannot_create_a_report_tree() {
    let fixture = Fixture::new();
    fixture.publish_run(RUN_A, &[RUN_B]);
    fixture.publish_run(RUN_B, &[RUN_A]);
    let original = fs::read(fixture.record_path()).unwrap();
    assert!(
        failure(fixture.save_output(1, json!({"nodes": [node(RUN_A, "A"), node(RUN_B, "B")]})))
            .contains("cycle")
    );
    assert_eq!(fs::read(fixture.record_path()).unwrap(), original);
    fixture.publish_run(RUN_B, &[]);
    fixture.publish_run(RUN_C, &[RUN_B]);
    assert!(
        failure(fixture.save_output(1, json!({"nodes": [node(RUN_B, "B")]})))
            .contains("more than one")
    );
    assert_eq!(fs::read(fixture.record_path()).unwrap(), original);
}

#[test]
fn analysis_compare_and_swap_rejects_old_revision_without_overwrite() {
    let fixture = Fixture::new();
    fixture.save(json!({"goal": goal("已更新问题")}));
    let original = fs::read(fixture.record_path()).unwrap();
    assert!(
        failure(fixture.save_output(1, json!({"goal": goal("旧写覆盖")})))
            .contains("revision conflict")
    );
    assert_eq!(fs::read(fixture.record_path()).unwrap(), original);
}

#[test]
fn analysis_concurrent_creators_publish_exactly_one_revision_winner() {
    concurrent_save(Fixture::empty(), 0);
}

#[test]
fn analysis_concurrent_writers_publish_exactly_one_revision_winner() {
    concurrent_save(Fixture::new(), 1);
}

fn concurrent_save(fixture: Fixture, revision: u64) {
    let inputs: Vec<_> = (0..8)
        .map(|index| {
            fixture.input(
                &format!("concurrent-{index}.json"),
                json!({"goal": goal(&format!("writer-{index}"))}),
            )
        })
        .collect();
    let processes: Vec<_> = inputs
        .iter()
        .map(|input| {
            fixture
                .save_command(revision, input)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let mut winners = Vec::new();
    for process in processes {
        let output = wait_bounded(process);
        if output.status.success() {
            winners.push(success(output));
        } else {
            assert!(failure(output).contains("revision conflict"));
        }
    }
    assert_eq!(winners.len(), 1);
    assert_eq!(winners[0]["revision"], revision + 1);
    assert_eq!(fixture.show(), winners[0]);
}

#[test]
fn corrupt_unsupported_and_foreign_records_are_never_silently_recreated() {
    let fixture = Fixture::new();
    let original: Value =
        serde_json::from_slice(&fs::read(fixture.record_path()).unwrap()).unwrap();
    let mut old = original.clone();
    old["schema_version"] = json!(1);
    let mut future = original.clone();
    future["schema_version"] = json!(999);
    let mut foreign = original.clone();
    foreign["session_id"] = json!("019f6e00-0000-7000-8000-000000000199");
    for (bytes, expected) in [
        (b"{broken".to_vec(), "corrupt"),
        (serde_json::to_vec(&old).unwrap(), "unsupported"),
        (serde_json::to_vec(&future).unwrap(), "unsupported"),
        (serde_json::to_vec(&foreign).unwrap(), "another Session"),
    ] {
        fs::write(fixture.record_path(), &bytes).unwrap();
        assert!(failure(fixture.show_output()).contains(expected));
        for revision in [0, 1] {
            assert!(
                failure(fixture.save_output(revision, json!({"goal": Fixture::goal()})))
                    .contains(expected)
            );
            assert_eq!(fs::read(fixture.record_path()).unwrap(), bytes);
        }
    }
}

#[test]
fn archived_query_material_can_be_read_without_repeating_it_in_node_content() {
    let fixture = Fixture::new();
    fixture.publish_run(RUN_A, &[]);
    fixture.save(json!({"nodes": [node(RUN_A, "查询 M1 显示等待占比为 60%。")]}));
    // 实际查询采集由 archive 集成测试覆盖，这里验证跨任务读取与按需材料选择器。
    let material = json!({"kind": "query", "run_id": RUN_A, "sql": "select ratio from output.main", "outputs": ["main"], "columns": [{"name": "ratio", "type": "double"}], "ndjson": "{\"ratio\":0.6}\n"});
    let mut persisted: Value =
        serde_json::from_slice(&fs::read(fixture.record_path()).unwrap()).unwrap();
    persisted["materials"]["M1"] = material.clone();
    fs::write(
        fixture.record_path(),
        serde_json::to_vec(&persisted).unwrap(),
    )
    .unwrap();
    let restored = fixture.show();
    assert_eq!(
        restored["materials"]["M1"],
        json!({"kind":"query","run_id":RUN_A})
    );
    let detail = success(
        command(&fixture.home)
            .args([
                "analysis",
                "show",
                "--session",
                &fixture.session_id,
                "--material",
                "M1",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(detail["material"], material);
    fixture.save(json!({"report": {"content": "原始材料由 M1 保留。"}}));
    assert_eq!(fixture.show()["materials"], restored["materials"]);
}

#[test]
fn deleting_session_also_removes_the_analysis_record() {
    let fixture = Fixture::new();
    let external = fixture.temporary.path().join("external.txt");
    fs::write(&external, "外部来源").unwrap();
    success(
        command(&fixture.home)
            .args(["session", "delete", "--session", &fixture.session_id])
            .output()
            .unwrap(),
    );
    assert!(!fixture.session_path().exists());
    assert_eq!(fs::read_to_string(external).unwrap(), "外部来源");
}

#[test]
fn analysis_concurrent_readers_observe_only_complete_committed_records() {
    let fixture = Fixture::new();
    let start = std::sync::Barrier::new(2);
    thread::scope(|scope| {
        scope.spawn(|| {
            start.wait();
            for previous_revision in 1..=12 {
                let result = success(fixture.save_output(
                    previous_revision,
                    json!({"goal": goal(&format!("revision-{}", previous_revision + 1))}),
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
    // 允许读取但拒绝替换需要的 delete sharing，制造真实提交故障。
    let held_record = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(fixture.record_path())
        .unwrap();
    failure(fixture.save_output(1, json!({"goal": goal("不能提交的目标")})));
    drop(held_record);
    assert_eq!(fs::read(fixture.record_path()).unwrap(), original);
    assert_eq!(fixture.show()["revision"], 1);
    assert_eq!(
        fixture.save(json!({"goal": goal("恢复后可以提交")}))["revision"],
        2
    );
}

#[test]
fn committed_analysis_survives_failure_to_publish_response_to_closed_stdout() {
    let fixture = Fixture::new();
    let question = "完整提交后的结论".repeat(32 * 1024);
    let input = fixture.input("closed-pipe.json", json!({"goal": goal(&question)}));
    let mut child = fixture
        .save_command(1, &input)
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
