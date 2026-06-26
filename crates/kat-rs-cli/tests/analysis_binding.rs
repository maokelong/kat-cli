use kat_rs_cli::trace_runtime::analysis::{binding::resolve_template, context::AnalysisState};
use serde_json::json;

#[test]
fn binding_resolves_params_and_state_paths() {
    let params = json!({
        "marker": "firstDrawFrame:1",
        "target_process": ".tencent.wechat"
    });
    let state = json!({
        "root": {
            "itid": 405,
            "start_ts": 246307034375i64,
            "end_ts": 246329389063i64
        }
    });

    assert_eq!(
        resolve_template("${params.marker}", &params, &state).expect("marker"),
        json!("firstDrawFrame:1")
    );
    assert_eq!(
        resolve_template("${state.root.itid}", &params, &state).expect("itid"),
        json!(405)
    );
    assert_eq!(
        resolve_template("prefix-${params.marker}", &params, &state).expect("inline"),
        json!("prefix-firstDrawFrame:1")
    );
}

#[test]
fn analysis_state_sets_nested_paths_without_losing_existing_fields() {
    let mut state = AnalysisState::default();
    state.set_path("root.itid", json!(405)).expect("set itid");
    state
        .set_path("root.window.start_ts", json!(246307034375i64))
        .expect("set start");

    assert_eq!(state.value()["root"]["itid"], json!(405));
    assert_eq!(
        state.value()["root"]["window"]["start_ts"],
        json!(246307034375i64)
    );
}
