use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use std::path::{Path, PathBuf};

fn create_run(out: &Path) -> PathBuf {
    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "init",
        "--out",
        out.join("runs").to_str().unwrap(),
        "--trace",
        "sample.htrace",
        "--question",
        "why slow",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"current_stage\":\"collect_input\""));

    PathBuf::from(fs::read_to_string(out.join(".last-run")).unwrap())
}

fn advance_stage(run_dir: &Path, from: &str, to: &str) {
    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "advance",
        run_dir.to_str().unwrap(),
        "--from",
        from,
        "--to",
        to,
        "--json",
    ]);
    cmd.assert().success();
}

fn advance_stage_with_decision(run_dir: &Path, from: &str, to: &str, decision: &str) {
    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "advance",
        run_dir.to_str().unwrap(),
        "--from",
        from,
        "--to",
        to,
        "--decision",
        decision,
        "--json",
    ]);
    cmd.assert().success();
}

fn advance_to_final_report(run_dir: &Path) {
    advance_stage(run_dir, "collect_input", "load_profile");
    advance_stage_with_decision(
        run_dir,
        "load_profile",
        "overview_atomics",
        "scheduler-kernel",
    );
    fs::create_dir_all(run_dir.join("evidence/overview")).unwrap();
    fs::write(
        run_dir.join("evidence/overview/trace_sanity_check.json"),
        "{}",
    )
    .unwrap();
    advance_stage(run_dir, "overview_atomics", "topdown_brief");
    fs::write(run_dir.join("artifacts/topdown-brief.md"), "brief").unwrap();
    advance_stage(run_dir, "topdown_brief", "strategy_selection");
    advance_stage_with_decision(
        run_dir,
        "strategy_selection",
        "deep_analysis",
        "cold-start-scheduler-topdown",
    );
    fs::create_dir_all(run_dir.join("evidence/deep")).unwrap();
    fs::write(run_dir.join("evidence/deep/thread_state.json"), "{}").unwrap();
    advance_stage(run_dir, "deep_analysis", "replay_generation");
    fs::write(run_dir.join("artifacts/replay.yaml"), "steps: []").unwrap();
    advance_stage(run_dir, "replay_generation", "final_report");
}

#[test]
fn run_init_json_reports_collect_input() {
    let dir = tempfile::tempdir().unwrap();

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "init",
        "--out",
        dir.path().join("runs").to_str().unwrap(),
        "--trace",
        "sample.htrace",
        "--question",
        "why slow",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"current_stage\":\"collect_input\""));
}

#[test]
fn run_status_reports_current_stage() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args(["run", "status", run_dir.to_str().unwrap(), "--json"]);
    cmd.assert()
        .success()
        .stdout(contains("\"current_stage\":\"collect_input\""))
        .stdout(contains("complete_input"));
}

#[test]
fn run_advance_moves_to_next_stage_and_updates_progress() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "advance",
        run_dir.to_str().unwrap(),
        "--from",
        "collect_input",
        "--to",
        "load_profile",
        "--decision",
        "input collected",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"advanced\":true"))
        .stdout(contains("\"to\":\"load_profile\""));

    let progress = fs::read_to_string(run_dir.join("progress.md")).unwrap();
    assert!(progress.contains("## 当前阶段 / Current Stage\n\nload_profile: 加载 profile"));
    assert!(progress.contains("## 已完成 / Completed\n\n- collect_input: 收集输入"));
    assert!(progress.contains("## 正在进行 / In Progress\n\n- load_profile: 加载 profile"));
}

#[test]
fn run_advance_rejects_missing_artifact_without_recording_it() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "advance",
        run_dir.to_str().unwrap(),
        "--from",
        "collect_input",
        "--to",
        "load_profile",
        "--artifact",
        "missing.md",
        "--json",
    ]);
    cmd.assert().failure().stderr(contains("artifact 不存在"));

    let state = fs::read_to_string(run_dir.join("run-state.yaml")).unwrap();
    assert!(!state.contains("missing.md"));
}

#[test]
fn run_advance_completed_requires_final_report_artifact() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());
    advance_to_final_report(&run_dir);

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "advance",
        run_dir.to_str().unwrap(),
        "--from",
        "final_report",
        "--to",
        "completed",
        "--json",
    ]);
    cmd.assert()
        .failure()
        .stderr(contains("缺少 artifacts/final-report.md"));
}

#[test]
fn run_advance_completed_marks_run_completed() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());
    advance_to_final_report(&run_dir);
    fs::write(
        run_dir.join("artifacts").join("final-report.md"),
        "final report",
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "advance",
        run_dir.to_str().unwrap(),
        "--from",
        "final_report",
        "--to",
        "completed",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"advanced\":true"))
        .stdout(contains("\"to\":\"completed\""));

    let mut status = Command::cargo_bin("htrace").unwrap();
    status.args(["run", "status", run_dir.to_str().unwrap(), "--json"]);
    status
        .assert()
        .success()
        .stdout(contains("\"status\":\"completed\""))
        .stdout(contains("\"next_allowed\":[]"));
}

#[test]
fn run_guard_json_blocks_final_report_before_workflow_reaches_it() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "guard",
        run_dir.to_str().unwrap(),
        "--action",
        "write_final_report",
        "--json",
    ]);
    cmd.assert()
        .failure()
        .stdout(contains("\"allowed\":false"))
        .stdout(contains("write_final_report"))
        .stderr(contains(
            "当前阶段 collect_input 不允许动作 write_final_report",
        ));
}

#[test]
fn run_guard_json_allows_current_stage_action_with_zero_exit() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "guard",
        run_dir.to_str().unwrap(),
        "--action",
        "complete_input",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"allowed\":true"))
        .stdout(contains("complete_input"));
}

#[test]
fn run_go_json_reports_stage_metadata_in_chinese() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args(["run", "go", run_dir.to_str().unwrap(), "--json"]);
    cmd.assert()
        .success()
        .stdout(contains("\"next_action\":\"open_stage\""))
        .stdout(contains("\"current_stage\":\"collect_input\""))
        .stdout(contains("\"name\":\"收集输入\""))
        .stdout(contains("complete_input"))
        .stdout(contains("run-state.yaml"));
}

#[test]
fn run_validate_json_reports_missing_overview_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());
    advance_stage(&run_dir, "collect_input", "load_profile");
    advance_stage_with_decision(
        &run_dir,
        "load_profile",
        "overview_atomics",
        "scheduler-kernel",
    );

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args(["run", "validate", run_dir.to_str().unwrap(), "--json"]);
    cmd.assert()
        .success()
        .stdout(contains("\"ok\":false"))
        .stdout(contains("\"code\":\"HT201\""))
        .stdout(contains("overview_atomics 阶段缺少 overview evidence"));
}

#[test]
fn run_go_json_is_blocked_when_current_stage_has_error_finding() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());
    let state_path = run_dir.join("run-state.yaml");
    let state = fs::read_to_string(&state_path).unwrap();
    fs::write(
        &state_path,
        state.replace(
            "current_stage: collect_input",
            "current_stage: overview_atomics",
        ),
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args(["run", "go", run_dir.to_str().unwrap(), "--json"]);
    cmd.assert()
        .success()
        .stdout(contains("\"next_action\":\"blocked\""))
        .stdout(contains("\"code\":\"HT101\""));
}

#[test]
fn run_advance_blocks_overview_without_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());
    advance_stage(&run_dir, "collect_input", "load_profile");
    advance_stage_with_decision(
        &run_dir,
        "load_profile",
        "overview_atomics",
        "scheduler-kernel",
    );

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "advance",
        run_dir.to_str().unwrap(),
        "--from",
        "overview_atomics",
        "--to",
        "topdown_brief",
        "--json",
    ]);
    cmd.assert()
        .failure()
        .stderr(contains("overview_atomics 阶段缺少 overview evidence"));
}

#[test]
fn run_advance_blocks_load_profile_without_selected_profile() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());
    advance_stage(&run_dir, "collect_input", "load_profile");

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "advance",
        run_dir.to_str().unwrap(),
        "--from",
        "load_profile",
        "--to",
        "overview_atomics",
        "--json",
    ]);
    cmd.assert().failure().stderr(contains(
        "load_profile 阶段缺少 profile.selected 或 profile.router_result",
    ));
}

#[test]
fn run_advance_uses_load_profile_decision_as_selected_profile() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());
    advance_stage(&run_dir, "collect_input", "load_profile");

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "advance",
        run_dir.to_str().unwrap(),
        "--from",
        "load_profile",
        "--to",
        "overview_atomics",
        "--decision",
        "scheduler-kernel",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"from\":\"load_profile\""))
        .stdout(contains("\"to\":\"overview_atomics\""));

    let state = fs::read_to_string(run_dir.join("run-state.yaml")).unwrap();
    assert!(state.contains("selected: scheduler-kernel"));

    let mut go = Command::cargo_bin("htrace").unwrap();
    go.args(["run", "go", run_dir.to_str().unwrap(), "--json"]);
    go.assert()
        .success()
        .stdout(contains("\"next_action\":\"open_stage\""))
        .stdout(contains("\"current_stage\":\"overview_atomics\""));
}

#[test]
fn run_advance_allows_overview_with_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());
    advance_stage(&run_dir, "collect_input", "load_profile");
    advance_stage_with_decision(
        &run_dir,
        "load_profile",
        "overview_atomics",
        "scheduler-kernel",
    );
    fs::create_dir_all(run_dir.join("evidence/overview")).unwrap();
    fs::write(
        run_dir.join("evidence/overview/trace_sanity_check.json"),
        "{}",
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "advance",
        run_dir.to_str().unwrap(),
        "--from",
        "overview_atomics",
        "--to",
        "topdown_brief",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"to\":\"topdown_brief\""));
}

#[test]
fn run_advance_uses_strategy_decision_from_current_command() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());
    advance_stage(&run_dir, "collect_input", "load_profile");
    advance_stage_with_decision(
        &run_dir,
        "load_profile",
        "overview_atomics",
        "scheduler-kernel",
    );
    fs::create_dir_all(run_dir.join("evidence/overview")).unwrap();
    fs::write(
        run_dir.join("evidence/overview/trace_sanity_check.json"),
        "{}",
    )
    .unwrap();
    advance_stage(&run_dir, "overview_atomics", "topdown_brief");
    fs::write(run_dir.join("artifacts/topdown-brief.md"), "brief").unwrap();
    advance_stage(&run_dir, "topdown_brief", "strategy_selection");

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "advance",
        run_dir.to_str().unwrap(),
        "--from",
        "strategy_selection",
        "--to",
        "deep_analysis",
        "--decision",
        "cold-start-scheduler-topdown",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"from\":\"strategy_selection\""))
        .stdout(contains("\"to\":\"deep_analysis\""));

    let state = fs::read_to_string(run_dir.join("run-state.yaml")).unwrap();
    assert!(state.contains("stage: strategy_selection"));
    assert!(state.contains("value: cold-start-scheduler-topdown"));
}

#[test]
fn run_advance_blocks_deep_analysis_without_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = create_run(dir.path());
    advance_stage(&run_dir, "collect_input", "load_profile");
    advance_stage_with_decision(
        &run_dir,
        "load_profile",
        "overview_atomics",
        "scheduler-kernel",
    );
    fs::create_dir_all(run_dir.join("evidence/overview")).unwrap();
    fs::write(
        run_dir.join("evidence/overview/trace_sanity_check.json"),
        "{}",
    )
    .unwrap();
    advance_stage(&run_dir, "overview_atomics", "topdown_brief");
    fs::write(run_dir.join("artifacts/topdown-brief.md"), "brief").unwrap();
    advance_stage(&run_dir, "topdown_brief", "strategy_selection");
    advance_stage_with_decision(
        &run_dir,
        "strategy_selection",
        "deep_analysis",
        "cold-start-scheduler-topdown",
    );

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "run",
        "advance",
        run_dir.to_str().unwrap(),
        "--from",
        "deep_analysis",
        "--to",
        "replay_generation",
        "--json",
    ]);
    cmd.assert()
        .failure()
        .stderr(contains("deep_analysis 阶段缺少 deep evidence"));
}
