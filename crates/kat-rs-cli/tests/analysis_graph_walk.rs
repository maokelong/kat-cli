use kat_rs_cli::trace_runtime::{
    analysis::{context::AnalysisState, steps::graph_walk::run_graph_walk_on_rows},
    pack::spec::{AnalysisStepSpec, ConditionOp},
};
use serde_json::json;

#[test]
fn graph_walk_selects_yaml_configured_self_execution_edge() {
    let yaml = r#"
id: walk_dependencies
kind: temporal.graph_walk
root:
  fromState: root
limits:
  maxDepth: 1
  maxEdgesPerNode: 1
edgeProviders:
  - id: self_execution
    table: thread_state_profile
    when:
      dominant_state:
        eq: Running
      dominant_percent:
        gte: 70
    emit:
      edgeType: self_execution
      target:
        sameNode: true
      evidence:
        - thread_state_profile
"#;
    let step: AnalysisStepSpec = serde_yaml::from_str(yaml).expect("graph walk step");
    let AnalysisStepSpec::TemporalGraphWalk(step) = step else {
        panic!("expected graph walk step");
    };
    assert_eq!(
        step.edge_providers[0].when["dominant_percent"],
        ConditionOp::Gte(70.0)
    );

    let mut state = AnalysisState::default();
    state.set_path("root.itid", json!(405)).expect("root itid");
    state
        .set_path("root.start_ts", json!(10))
        .expect("root start");
    state.set_path("root.end_ts", json!(30)).expect("root end");

    let evidence = run_graph_walk_on_rows(
        &step,
        &mut state,
        &[(
            "thread_state_profile",
            vec![json!({
                "itid": 405,
                "dominant_state": "Running",
                "dominant_percent": 95.0
            })],
        )],
    )
    .expect("graph walk");

    assert_eq!(state.value()["decisions"][0]["edgeType"], "self_execution");
    assert_eq!(
        state.value()["visitedEdges"][0]["edgeType"],
        "self_execution"
    );
    assert_eq!(evidence[0]["facts"]["selectedEdgeType"], "self_execution");
}

#[test]
fn graph_walk_records_uncertainty_when_no_provider_matches() {
    let yaml = r#"
id: walk_dependencies
kind: temporal.graph_walk
root:
  fromState: root
limits:
  maxDepth: 1
  maxEdgesPerNode: 1
edgeProviders:
  - id: self_execution
    table: thread_state_profile
    when:
      dominant_state:
        eq: Running
      dominant_percent:
        gte: 70
    emit:
      edgeType: self_execution
      target:
        sameNode: true
"#;
    let AnalysisStepSpec::TemporalGraphWalk(step) = serde_yaml::from_str(yaml).expect("step")
    else {
        panic!("expected graph walk step");
    };
    let mut state = AnalysisState::default();

    let evidence = run_graph_walk_on_rows(
        &step,
        &mut state,
        &[(
            "thread_state_profile",
            vec![json!({
                "dominant_state": "S",
                "dominant_percent": 95.0
            })],
        )],
    )
    .expect("graph walk");

    assert_eq!(state.value()["decisions"][0]["status"], "no_edge");
    assert_eq!(evidence[0]["status"], "partial");
    assert!(
        evidence[0]["limitations"][0]
            .as_str()
            .unwrap()
            .contains("No graph edge provider matched")
    );
}
