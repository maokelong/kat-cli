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
    assert_eq!(evidence["evidenceId"], "ev.seed_root.first_draw_window");
    assert_eq!(evidence["facts"]["vsync_id"], json!(3269));
    assert_eq!(evidence["tableRefs"], json!(["first_draw_window"]));
}
