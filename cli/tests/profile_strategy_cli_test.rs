use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn profile_list_prints_scheduler_kernel() {
    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args(["profile", "list", "--skill-root", "../skill"]);
    cmd.assert().success().stdout(contains("scheduler-kernel"));
}

#[test]
fn profile_route_uses_configured_aliases() {
    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "profile",
        "route",
        "--skill-root",
        "../skill",
        "--question",
        "冷启动主线程调度等待很高",
    ]);
    cmd.assert().success().stdout(contains("scheduler-kernel"));
}

#[test]
fn strategy_render_prints_markdown_body() {
    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "strategy",
        "render",
        "--skill-root",
        "../skill",
        "cold-start-scheduler-topdown",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("冷启动调度/内核 Topdown 策略"));
}

#[test]
fn strategy_lint_accepts_approved_strategy() {
    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "strategy",
        "lint",
        "--skill-root",
        "../skill",
        "../skill/strategies/approved/cold-start-scheduler-topdown.md",
    ]);
    cmd.assert().success().stdout(contains("ok"));
}
