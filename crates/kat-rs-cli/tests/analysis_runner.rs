use kat_rs_cli::trace_runtime::analysis::{
    context::AnalysisState,
    report::render_report,
    runner::{AnalysisRunConfig, run_analysis},
    steps::evidence::render_seed_evidence,
};
use kat_rs_cli::trace_runtime::pack::load_pack;
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn evidence_render_seeds_root_state_from_first_row() {
    let mut state = AnalysisState::default();
    let rows = vec![json!({
        "callstack_id": 30754,
        "root_callstack_id": 30493,
        "itid": 405,
        "process_name": ".tencent.wechat",
        "vsync_id": 3269,
        "start_ts": 246307034375i64,
        "end_ts": 246329389063i64,
        "dur_ns": 22354688i64
    })];

    let evidence = render_seed_evidence("seed_root", "first_draw_window", &rows, &mut state)
        .expect("evidence");

    assert_eq!(state.value()["root"]["itid"], json!(405));
    assert_eq!(state.value()["root"]["start_ts"], json!(246307034375i64));
    assert_eq!(
        state.value()["frontier"]["nodes"],
        json!([state.value()["root"].clone()])
    );
    assert_eq!(evidence["evidenceId"], "ev.seed_root.first_draw_window");
    assert_eq!(evidence["facts"]["vsync_id"], json!(3269));
    assert_eq!(evidence["tableRefs"], json!(["first_draw_window"]));
}

#[test]
fn evidence_render_rejects_empty_rows() {
    let mut state = AnalysisState::default();

    let error = render_seed_evidence("seed_root", "first_draw_window", &[], &mut state)
        .expect_err("empty rows should fail");

    assert!(error.to_string().contains("first_draw_window"));
}

#[test]
fn evidence_render_rejects_non_object_first_row() {
    let mut state = AnalysisState::default();
    let rows = vec![json!("not a row object")];

    let error = render_seed_evidence("seed_root", "first_draw_window", &rows, &mut state)
        .expect_err("non-object first row should fail");

    assert!(error.to_string().contains("first_draw_window"));
}

#[test]
fn evidence_render_rejects_empty_facts_without_mutating_state() {
    let mut state = AnalysisState::default();
    let default_state = AnalysisState::default();
    let rows = vec![json!({
        "tid": 12,
        "ipid": 34,
        "thread_name": "RenderThread"
    })];

    let error = render_seed_evidence("seed_root", "thread_only_window", &rows, &mut state)
        .expect_err("empty facts should fail");

    assert!(error.to_string().contains("thread_only_window"));
    assert_eq!(state.value(), default_state.value());
}

#[test]
fn report_renderer_separates_facts_inferences_and_uncertainty() {
    let state = json!({
        "root": {
            "process_name": ".tencent.wechat",
            "itid": 405,
            "vsync_id": 3269,
            "start_ts": 246307034375i64,
            "end_ts": 246329389063i64
        },
        "decisions": [
            {
                "status": "selected",
                "edgeType": "self_execution",
                "provider": "self_execution"
            }
        ]
    });
    let evidence = vec![
        json!({
            "evidenceId": "ev.thread_state_profile",
            "status": "ok",
            "facts": {
                "dominantState": "Running",
                "dominantPercent": 95.0
            },
            "tableRefs": ["thread_state_profile"],
            "limitations": []
        }),
        json!({
            "evidenceId": "ev.callstack_self_time",
            "status": "ok",
            "facts": {
                "topSpanName": "CreateImagePixelMap resource:///1140850711.png",
                "topSpanDurMs": 16.84
            },
            "tableRefs": ["callstack_self_time"],
            "limitations": []
        }),
        json!({
            "evidenceId": "ev.io_sample_overlap",
            "status": "partial",
            "facts": { "overlapRows": 0 },
            "tableRefs": ["io_sample_overlap"],
            "limitations": ["No matching IO samples"]
        }),
    ];

    let report = render_report(&state, &evidence).expect("report");

    assert!(report.contains("# Facts"));
    assert!(report.contains("# Inferences"));
    assert!(report.contains("# Uncertainty"));
    assert!(report.contains(".tencent.wechat"));
    assert!(report.contains("Running"));
    assert!(report.contains("CreateImagePixelMap resource:///1140850711.png"));
    assert!(report.contains("No matching IO samples"));
}

#[test]
fn runner_writes_plan_state_evidence_checklist_and_report() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    create_minimal_trace_fixture(&raw_db);

    let pack = load_pack(workspace_root().join("packs/openharmony-core")).expect("pack");
    let run_dir = dir.path().join("runs");

    run_analysis(AnalysisRunConfig {
        raw_db: raw_db.clone(),
        scratch_db: dir.path().join("scratch.db"),
        run_root: run_dir.clone(),
        run_id: "fixture-run".to_string(),
        pack,
        analysis_id: "openharmony.critical_path".to_string(),
        params: serde_json::json!({
            "target_process": ".tencent.wechat",
            "marker": "firstDrawFrame:1"
        }),
    })
    .expect("analysis run");

    let output = run_dir.join("fixture-run");
    assert!(output.join("plan.json").is_file());
    assert!(output.join("state.json").is_file());
    assert!(output.join("evidence.jsonl").is_file());
    assert!(output.join("checklist.md").is_file());
    assert!(output.join("report.md").is_file());

    let report = std::fs::read_to_string(output.join("report.md")).expect("report");
    assert!(report.contains("# Facts"));
    assert!(report.contains("# Inferences"));
    assert!(report.contains("# Uncertainty"));
    assert!(report.contains(".tencent.wechat"));
}

fn create_minimal_trace_fixture(path: &std::path::Path) {
    let conn = Connection::open(path).expect("raw db");
    conn.execute_batch(
        "
        CREATE TABLE process (ipid INTEGER, pid INTEGER, name TEXT);
        CREATE TABLE thread (
            itid INTEGER,
            tid INTEGER,
            ipid INTEGER,
            thread_name TEXT,
            is_main_thread INTEGER
        );
        CREATE TABLE callstack (
            id INTEGER,
            callid INTEGER,
            parent_id INTEGER,
            name TEXT,
            ts INTEGER,
            dur INTEGER
        );
        CREATE TABLE thread_state (itid INTEGER, ts INTEGER, dur INTEGER, state TEXT);
        CREATE TABLE frame_slice (
            id INTEGER,
            itid INTEGER,
            ipid INTEGER,
            ts INTEGER,
            dur INTEGER,
            src TEXT
        );
        CREATE TABLE file_system_sample (ts INTEGER, dur INTEGER, name TEXT);
        CREATE TABLE bio_latency_sample (ts INTEGER, dur INTEGER, name TEXT);
        CREATE TABLE diskio (ts INTEGER, dur INTEGER, name TEXT);
        CREATE TABLE syscall (ts INTEGER, dur INTEGER, name TEXT);
        CREATE TABLE instant (ts INTEGER, name TEXT, ref INTEGER, wakeup_from INTEGER);

        INSERT INTO process VALUES (7, 1001, '.tencent.wechat');
        INSERT INTO process VALUES (8, 1002, 'render_service');
        INSERT INTO thread VALUES (405, 1001, 7, 'main', 1);
        INSERT INTO thread VALUES (406, 1002, 8, 'RenderThread', 0);
        INSERT INTO callstack VALUES (
            30754,
            405,
            NULL,
            'firstDrawFrame:1 [vsyncID:3269] [layoutMeasureDurationStartTimestamp:1000] [layoutMeasureDurationEndTimestamp:3000]',
            900,
            2200
        );
        INSERT INTO callstack VALUES (40000, 406, NULL, 'RenderService::DrawFrame', 3050, 1000);
        INSERT INTO thread_state VALUES (405, 1000, 2000, 'Running');
        INSERT INTO frame_slice VALUES (500, 405, 7, 900, 2200, '');
        INSERT INTO frame_slice VALUES (501, 406, 8, 3050, 1000, 'app_frame=500');
        INSERT INTO instant VALUES (1500, 'sched_wakeup', 405, 406);
        ",
    )
    .expect("fixture schema");
}

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf()
}
