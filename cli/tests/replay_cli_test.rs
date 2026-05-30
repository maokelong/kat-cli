use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use tempfile::tempdir;

#[test]
fn replay_run_executes_mock_steps() {
    let dir = tempdir().unwrap();
    let replay = dir.path().join("replay.yaml");
    fs::write(
        &replay,
        r#"
problem_signature: cold_start_sched_latency_v1
source_strategy: cold-start-scheduler-topdown
steps:
  - atomic: trace_sanity_check
    params: {}
"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "replay",
        "run",
        replay.to_str().unwrap(),
        "--skill-root",
        "../skill",
        "--trace",
        "sample.pftrace",
        "--engine",
        "mock",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains(
            "\"problem_signature\":\"cold_start_sched_latency_v1\"",
        ))
        .stdout(contains("\"step_count\":1"));
}
