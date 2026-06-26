use kat_rs_cli::trace_runtime::analysis::{
    context::AnalysisState, steps::evidence::render_seed_evidence,
};
use serde_json::json;

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
