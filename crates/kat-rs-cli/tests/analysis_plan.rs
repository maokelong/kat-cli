use kat_rs_cli::trace_runtime::pack::spec::{AnalysisSpec, AnalysisStepSpec, ConditionOp};

#[test]
fn critical_path_plan_parses_typed_steps_and_graph_walk_config() {
    let yaml = r#"
id: openharmony.critical_path
inputs:
  target_process:
    required: true
  marker:
    default: firstDrawFrame:1
requires:
  derived:
    - first_draw_window
steps:
  - id: seed_root
    kind: evidence.render
    from: first_draw_window
    writes:
      state.root: first_row
  - id: walk_dependencies
    kind: temporal.graph_walk
    root:
      fromState: root
    limits:
      maxDepth: 3
      maxEdgesPerNode: 2
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
  - id: render_report
    kind: report.render
"#;

    let spec: AnalysisSpec = serde_yaml::from_str(yaml).expect("typed analysis plan");

    assert_eq!(spec.id, "openharmony.critical_path");
    assert!(spec.inputs["target_process"].required);
    assert_eq!(
        spec.inputs["marker"].default.as_deref(),
        Some("firstDrawFrame:1")
    );
    assert_eq!(spec.requires.derived, vec!["first_draw_window"]);

    match &spec.steps[0] {
        AnalysisStepSpec::EvidenceRender(step) => {
            assert_eq!(step.id, "seed_root");
            assert_eq!(step.from, "first_draw_window");
            assert_eq!(step.writes["state.root"], "first_row");
        }
        other => panic!("expected evidence.render, got {other:?}"),
    }

    match &spec.steps[1] {
        AnalysisStepSpec::TemporalGraphWalk(step) => {
            assert_eq!(step.root.from_state, "root");
            assert_eq!(step.limits.max_depth, 3);
            assert_eq!(step.edge_providers[0].id, "self_execution");
            assert_eq!(
                step.edge_providers[0].when["dominant_percent"],
                ConditionOp::Gte(70.0)
            );
            assert!(step.edge_providers[0].emit.target.same_node);
        }
        other => panic!("expected temporal.graph_walk, got {other:?}"),
    }

    assert!(matches!(spec.steps[2], AnalysisStepSpec::ReportRender(_)));
}

#[test]
fn graph_walk_provider_target_parses_field_references() {
    let yaml = r#"
id: openharmony.critical_path
steps:
  - id: walk_dependencies
    kind: temporal.graph_walk
    root:
      fromState: root
    edgeProviders:
      - id: wakeup
        table: sched_wakeup
        emit:
          edgeType: wakeup
          target:
            itid: waker_itid
            start_ts: wake_ts
            end_ts: wake_ts
"#;

    let spec: AnalysisSpec = serde_yaml::from_str(yaml).expect("target field references");

    let AnalysisStepSpec::TemporalGraphWalk(step) = &spec.steps[0] else {
        panic!("expected temporal.graph_walk");
    };
    let target = &step.edge_providers[0].emit.target;
    assert_eq!(target.itid.as_deref(), Some("waker_itid"));
    assert_eq!(target.start_ts.as_deref(), Some("wake_ts"));
    assert_eq!(target.end_ts.as_deref(), Some("wake_ts"));
    assert_eq!(step.limits.max_depth, 3);
    assert_eq!(step.limits.max_edges_per_node, 3);
}

#[test]
fn graph_walk_provider_target_accepts_camel_case_timestamp_aliases() {
    let yaml = r#"
id: openharmony.critical_path
steps:
  - id: walk_dependencies
    kind: temporal.graph_walk
    root:
      fromState: root
    edgeProviders:
      - id: downstream
        table: slices
        emit:
          edgeType: downstream
          target:
            itid: target_itid
            startTs: start_ts
            endTs: end_ts
"#;

    let spec: AnalysisSpec = serde_yaml::from_str(yaml).expect("camelCase target aliases");

    let AnalysisStepSpec::TemporalGraphWalk(step) = &spec.steps[0] else {
        panic!("expected temporal.graph_walk");
    };
    let target = &step.edge_providers[0].emit.target;
    assert_eq!(target.itid.as_deref(), Some("target_itid"));
    assert_eq!(target.start_ts.as_deref(), Some("start_ts"));
    assert_eq!(target.end_ts.as_deref(), Some("end_ts"));
}

#[test]
fn execution_critical_fields_are_required() {
    for yaml in [
        r#"
id: missing.evidence_from
steps:
  - id: seed_root
    kind: evidence.render
"#,
        r#"
id: missing.graph_root
steps:
  - id: walk_dependencies
    kind: temporal.graph_walk
"#,
        r#"
id: missing.graph_root_from_state
steps:
  - id: walk_dependencies
    kind: temporal.graph_walk
    root: {}
"#,
    ] {
        let error = serde_yaml::from_str::<AnalysisSpec>(yaml)
            .expect_err("missing execution-critical field should fail");
        assert!(
            error.to_string().contains("missing field"),
            "unexpected error: {error}"
        );
    }
}
