use kat_rs_cli::trace_runtime::analysis::run_store::AnalysisRunStore;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn run_store_writes_plan_state_evidence_checklist_and_report() {
    let dir = tempdir().expect("tempdir");
    let store = AnalysisRunStore::create(dir.path(), "run-1").expect("run store");

    store.write_plan(&json!({"runId": "run-1"})).expect("plan");
    store
        .write_state(&json!({"frontier": {"nextCandidateEdges": []}}))
        .expect("state");
    store
        .append_evidence(&json!({"evidenceId": "ev.1"}))
        .expect("evidence");
    store.render_checklist().expect("checklist");
    store.write_report("# Facts\n\n- rows: 2\n").expect("report");

    assert!(dir.path().join("run-1/plan.json").is_file());
    assert!(dir.path().join("run-1/state.json").is_file());
    assert!(dir.path().join("run-1/evidence.jsonl").is_file());
    assert!(dir.path().join("run-1/checklist.md").is_file());
    assert!(dir.path().join("run-1/report.md").is_file());
    let checklist =
        std::fs::read_to_string(dir.path().join("run-1/checklist.md")).expect("checklist text");
    assert!(checklist.contains("plan.json"), "{checklist}");
    assert!(checklist.contains("state.json"), "{checklist}");
    assert!(checklist.contains("evidence.jsonl"), "{checklist}");
}

#[test]
fn create_rejects_run_ids_that_escape_root() {
    let dir = tempdir().expect("tempdir");
    assert!(AnalysisRunStore::create(dir.path(), "../escape").is_err());
    assert!(AnalysisRunStore::create(dir.path(), "/tmp/escape").is_err());
    #[cfg(windows)]
    assert!(AnalysisRunStore::create(dir.path(), "C:\\escape").is_err());
}

#[test]
fn append_evidence_writes_one_json_value_per_line() {
    let dir = tempdir().expect("tempdir");
    let store = AnalysisRunStore::create(dir.path(), "run-1").expect("run store");
    store
        .append_evidence(&json!({"evidenceId": "ev.1"}))
        .expect("first");
    store
        .append_evidence(&json!({"evidenceId": "ev.2"}))
        .expect("second");

    let raw =
        std::fs::read_to_string(dir.path().join("run-1/evidence.jsonl")).expect("evidence text");
    let lines = raw.lines().collect::<Vec<_>>();
    assert_eq!(2, lines.len());
    assert_eq!(
        "ev.1",
        serde_json::from_str::<serde_json::Value>(lines[0]).expect("line 1")["evidenceId"]
    );
    assert_eq!(
        "ev.2",
        serde_json::from_str::<serde_json::Value>(lines[1]).expect("line 2")["evidenceId"]
    );
    assert!(raw.ends_with('\n'));
}

#[test]
fn append_evidence_preserves_line_boundary_after_preexisting_line_without_newline() {
    let dir = tempdir().expect("tempdir");
    let store = AnalysisRunStore::create(dir.path(), "run-1").expect("run store");
    let evidence_path = dir.path().join("run-1/evidence.jsonl");
    std::fs::write(&evidence_path, r#"{"evidenceId":"ev.preexisting"}"#).expect("seed");

    store
        .append_evidence(&json!({"evidenceId": "ev.appended"}))
        .expect("append");

    let raw = std::fs::read_to_string(evidence_path).expect("evidence text");
    let lines = raw.lines().collect::<Vec<_>>();
    assert_eq!(2, lines.len());
    assert_eq!(
        "ev.preexisting",
        serde_json::from_str::<serde_json::Value>(lines[0]).expect("line 1")["evidenceId"]
    );
    assert_eq!(
        "ev.appended",
        serde_json::from_str::<serde_json::Value>(lines[1]).expect("line 2")["evidenceId"]
    );
    assert!(raw.ends_with('\n'));
}
