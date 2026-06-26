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
      score: dominant_percent
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
    assert_eq!(state.value()["visitedEdges"][0]["score"], 95.0);
    assert_eq!(evidence[0]["facts"]["selectedEdgeType"], "self_execution");
}

#[test]
fn graph_walk_emits_yaml_configured_facts_from_generic_tables() {
    let yaml = r#"
id: walk_dependencies
kind: temporal.graph_walk
root:
  fromState: root
limits:
  maxDepth: 1
  maxEdgesPerNode: 1
edgeProviders:
  - id: chosen_path
    table: candidate_edges
    when:
      ready:
        eq: true
    emit:
      edgeType: chosen
      target:
        sameNode: true
      evidence:
        - candidate_edges
        - detail_rows
      facts:
        copiedLabel:
          field: label
        matchedDurationMs:
          table: detail_rows
          field: raw_duration_ns
          scale: 0.000001
          row:
            where:
              rank:
                eq: 1
            fallback: first
        detailCount:
          table: detail_rows
          count: true
"#;
    let AnalysisStepSpec::TemporalGraphWalk(step) = serde_yaml::from_str(yaml).expect("step")
    else {
        panic!("expected graph walk step");
    };
    let mut state = AnalysisState::default();

    let evidence = run_graph_walk_on_rows(
        &step,
        &mut state,
        &[
            (
                "candidate_edges",
                vec![json!({
                    "ready": true,
                    "label": "provider-row"
                })],
            ),
            (
                "detail_rows",
                vec![
                    json!({
                        "name": "winner",
                        "raw_duration_ns": 4200000,
                        "rank": 2
                    }),
                    json!({
                        "name": "selected detail",
                        "raw_duration_ns": 16840000,
                        "rank": 1
                    }),
                ],
            ),
        ],
    )
    .expect("graph walk");

    assert_eq!(evidence[0]["facts"]["selectedEdgeType"], "chosen");
    assert_eq!(evidence[0]["facts"]["provider"], "chosen_path");
    assert_eq!(evidence[0]["facts"]["matchedTable"], "candidate_edges");
    assert_eq!(evidence[0]["facts"]["copiedLabel"], "provider-row");
    assert_eq!(evidence[0]["facts"]["matchedDurationMs"], 16.84);
    assert_eq!(evidence[0]["facts"]["detailCount"], 2);
}

#[test]
fn graph_walk_uses_yaml_configured_generic_score_field() {
    let yaml = r#"
id: walk_dependencies
kind: temporal.graph_walk
root:
  fromState: root
limits:
  maxDepth: 1
  maxEdgesPerNode: 1
edgeProviders:
  - id: custom_provider
    table: custom_edges
    when:
      edge_ready:
        eq: true
    emit:
      edgeType: custom_edge
      score: custom_priority
      target:
        itid: custom_itid
"#;
    let AnalysisStepSpec::TemporalGraphWalk(step) = serde_yaml::from_str(yaml).expect("step")
    else {
        panic!("expected graph walk step");
    };
    let mut state = AnalysisState::default();

    run_graph_walk_on_rows(
        &step,
        &mut state,
        &[(
            "custom_edges",
            vec![json!({
                "edge_ready": true,
                "custom_itid": 777,
                "custom_priority": 0.42
            })],
        )],
    )
    .expect("graph walk");

    assert_eq!(state.value()["visitedEdges"][0]["edgeType"], "custom_edge");
    assert_eq!(state.value()["visitedEdges"][0]["score"], 0.42);
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
