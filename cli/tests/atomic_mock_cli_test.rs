use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn atomic_run_with_mock_engine_returns_json_envelope() {
    let mut cmd = Command::cargo_bin("htrace").unwrap();
    cmd.args([
        "atomic",
        "run",
        "--skill-root",
        "../skill",
        "--engine",
        "mock",
        "trace_sanity_check",
        "--trace",
        "sample.pftrace",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"status\":\"ok\""))
        .stdout(contains("\"atomic_id\":\"trace_sanity_check\""));
}
