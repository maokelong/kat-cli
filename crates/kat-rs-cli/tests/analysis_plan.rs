use kat_rs_cli::trace_runtime::pack::spec::{
    AnalysisSpec, AnalysisStepSpec, BindingExpr, GraphValueSpec,
};

#[test]
fn critical_path_plan_parses_typed_steps_and_graph_walk_config() {
    let yaml = std::fs::read_to_string(
        workspace_root().join("packs/openharmony-core/analyses/critical-path.plan.yaml"),
    )
    .expect("critical path plan");
    let spec: AnalysisSpec = serde_yaml::from_str(&yaml).expect("typed analysis plan");

    assert_eq!(spec.id, "openharmony.critical_path");
    assert!(spec.inputs["target_process"].required);
    assert_eq!(
        spec.inputs["marker"].default.as_deref(),
        Some("firstDrawFrame:1")
    );
    assert!(
        spec.requires
            .derived
            .contains(&"first_draw_window".to_string())
    );

    match &spec.steps[0] {
        AnalysisStepSpec::EvidenceRender(step) => {
            assert_eq!(step.id, "seed_root");
            assert_eq!(step.from, "first_draw_window");
            assert_eq!(step.writes["state.root"], "first_row");
        }
        other => panic!("expected evidence.render, got {other:?}"),
    }

    match &spec.steps[1] {
        AnalysisStepSpec::GraphWalk(step) => {
            assert_eq!(step.root.from_state, "root");
            assert_eq!(step.limits.max_depth, 3);
            assert_eq!(step.limits.max_nodes, 50);
            assert_eq!(
                step.limits.max_edges_per_node, 4,
                "critical path fanout must allow all four providers so self_top_span cannot crowd out downstream_frame"
            );
            let provider_ids = step
                .providers
                .iter()
                .map(|provider| provider.id.as_str())
                .collect::<Vec<_>>();
            assert_eq!(
                provider_ids,
                vec![
                    "self_execution",
                    "self_top_span",
                    "sleeping_wakeup",
                    "downstream_frame"
                ]
            );

            let self_execution = step
                .providers
                .iter()
                .find(|provider| provider.id == "self_execution")
                .expect("self_execution provider");
            assert_eq!(self_execution.input.table, "thread_state_profile");
            assert_eq!(self_execution.output.relation, "self_execution");
            assert_eq!(
                self_execution.output.evidence.tables,
                ["thread_state_profile"]
            );
            assert_eq!(
                self_execution.output.annotations["dominantState"],
                GraphValueSpec::Value(BindingExpr::Path("row.dominant_state".to_string()))
            );

            let sleeping_wakeup = step
                .providers
                .iter()
                .find(|provider| provider.id == "sleeping_wakeup")
                .expect("sleeping_wakeup provider");
            assert_eq!(
                sleeping_wakeup.expand.node.fields["start_ts"],
                GraphValueSpec::Value(BindingExpr::Path("row.wake_ts".to_string()))
            );
        }
        other => panic!("expected graph.walk, got {other:?}"),
    }

    assert!(matches!(spec.steps[2], AnalysisStepSpec::ReportRender(_)));
}

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf()
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
    ] {
        let error = serde_yaml::from_str::<AnalysisSpec>(yaml)
            .expect_err("missing execution-critical field should fail");
        assert!(
            error.to_string().contains("missing field"),
            "unexpected error: {error}"
        );
    }
}

#[test]
fn generic_graph_walk_requires_root_from_state() {
    for yaml in [
        r#"
id: missing.graph_root
steps:
  - id: walk_dependencies
    kind: graph.walk
"#,
        r#"
id: missing.graph_root_from_state
steps:
  - id: walk_dependencies
    kind: graph.walk
    root: {}
"#,
    ] {
        let error = serde_yaml::from_str::<AnalysisSpec>(yaml)
            .expect_err("missing generic graph root should fail");
        assert!(
            error.to_string().contains("missing field"),
            "unexpected error: {error}"
        );
    }
}

#[test]
fn generic_graph_walk_plan_parses_provider_pipeline() {
    use kat_rs_cli::trace_runtime::pack::spec::AnalysisStepSpec;

    let yaml = r#"
id: generic.problem_analysis
steps:
  - id: walk_dependencies
    kind: graph.walk
    root:
      fromState: root
    limits:
      maxDepth: 2
      maxNodes: 10
      maxEdgesPerNode: 3
    providers:
      - id: wakeup
        input:
          table: wakeup_edges
        match:
          all:
            - eq: [source.itid, row.target_itid]
            - temporal.pointWithin:
                point: row.wake_ts
                window:
                  start: source.start_ts
                  end: source.end_ts
        expand:
          node:
            fields:
              itid: row.waker_itid
              start_ts: row.wake_ts
              end_ts: row.wake_ts
        select:
          limit: 1
          orderBy:
            - expr: row.wait_ns
              desc: true
          dedupeBy:
            - node.itid
        output:
          relation: wakeup
          evidence:
            tables: [wakeup_edges]
          annotations:
            wakeTs: row.wake_ts
"#;

    let spec: kat_rs_cli::trace_runtime::pack::spec::AnalysisSpec =
        serde_yaml::from_str(yaml).expect("generic graph walk plan");

    let AnalysisStepSpec::GraphWalk(step) = &spec.steps[0] else {
        panic!("expected graph.walk step");
    };
    assert_eq!(step.id, "walk_dependencies");
    assert_eq!(step.root.from_state, "root");
    assert_eq!(step.limits.max_depth, 2);
    assert_eq!(step.limits.max_nodes, 10);
    assert_eq!(step.limits.max_edges_per_node, 3);
    assert_eq!(step.providers[0].id, "wakeup");
    assert_eq!(step.providers[0].input.table, "wakeup_edges");
    assert_eq!(step.providers[0].output.relation, "wakeup");
}

#[test]
fn generic_graph_walk_plan_parses_defaults_and_value_specs() {
    use kat_rs_cli::trace_runtime::pack::spec::{AnalysisStepSpec, BindingExpr, GraphValueSpec};

    let yaml = r#"
id: generic.problem_analysis
steps:
  - id: walk_dependencies
    kind: graph.walk
    root:
      fromState: root
    providers:
      - id: wakeup
        input:
          table: wakeup_edges
        match:
          all: []
        expand:
          node:
            sameAs: source
            fields:
              itid: row.waker_itid
              waitMs:
                value: row.wait_ns
                scale: 0.000001
        select:
          orderBy:
            - expr: row.wait_ns
              desc: true
          dedupeBy:
            - node.itid
            - node.start_ts
        output:
          relation: wakeup
          evidence:
            tables: [wakeup_edges, sched_slice]
          annotations:
            waitMs:
              value: row.wait_ns
              scale: 0.000001
"#;

    let spec: kat_rs_cli::trace_runtime::pack::spec::AnalysisSpec =
        serde_yaml::from_str(yaml).expect("generic graph walk defaults and values");

    let AnalysisStepSpec::GraphWalk(step) = &spec.steps[0] else {
        panic!("expected graph.walk step");
    };
    assert_eq!(step.limits.max_depth, 3);
    assert_eq!(step.limits.max_nodes, 50);
    assert_eq!(step.limits.max_edges_per_node, 3);

    let provider = &step.providers[0];
    assert_eq!(
        provider.expand.node.same_as.as_ref().unwrap(),
        &BindingExpr::Path("source".to_string())
    );
    assert_eq!(
        provider.expand.node.fields["itid"],
        GraphValueSpec::Value(BindingExpr::Path("row.waker_itid".to_string()))
    );
    let GraphValueSpec::Scaled { value, scale } = &provider.expand.node.fields["waitMs"] else {
        panic!("expected scaled node field");
    };
    assert_eq!(value, &BindingExpr::Path("row.wait_ns".to_string()));
    assert_eq!(*scale, 0.000001);

    assert_eq!(provider.select.order_by.len(), 1);
    assert_eq!(
        provider.select.order_by[0].expr,
        BindingExpr::Path("row.wait_ns".to_string())
    );
    assert!(provider.select.order_by[0].desc);
    assert_eq!(
        provider.select.dedupe_by[0],
        BindingExpr::Path("node.itid".to_string())
    );
    assert_eq!(
        provider.select.dedupe_by[1],
        BindingExpr::Path("node.start_ts".to_string())
    );
    assert_eq!(
        provider.output.evidence.tables,
        ["wakeup_edges", "sched_slice"]
    );

    let GraphValueSpec::Scaled { value, scale } = &provider.output.annotations["waitMs"] else {
        panic!("expected scaled annotation");
    };
    assert_eq!(value, &BindingExpr::Path("row.wait_ns".to_string()));
    assert_eq!(*scale, 0.000001);
}

#[test]
fn graph_value_spec_only_treats_exact_value_scale_object_as_scaled() {
    use kat_rs_cli::trace_runtime::pack::spec::{BindingExpr, GraphValueSpec};
    use serde_json::json;

    let value_only: GraphValueSpec = serde_yaml::from_str("value: x").expect("value-only object");
    assert_eq!(
        value_only,
        GraphValueSpec::Value(BindingExpr::Literal(json!({ "value": "x" })))
    );

    let scale_only: GraphValueSpec = serde_yaml::from_str("scale: 1.0").expect("scale-only object");
    assert_eq!(
        scale_only,
        GraphValueSpec::Value(BindingExpr::Literal(json!({ "scale": 1.0 })))
    );

    let extra_key: GraphValueSpec =
        serde_yaml::from_str("value: row.wait_ns\nscale: 1.0\nextra: true")
            .expect("extra-key object");
    assert_eq!(
        extra_key,
        GraphValueSpec::Value(BindingExpr::Literal(json!({
            "value": "row.wait_ns",
            "scale": 1.0,
            "extra": true
        })))
    );

    let malformed = serde_yaml::from_str::<GraphValueSpec>("value: row.wait_ns\nscale: bad")
        .expect_err("malformed scaled object should fail");
    assert!(
        malformed.to_string().contains("invalid type"),
        "unexpected error: {malformed}"
    );

    let scaled: GraphValueSpec =
        serde_yaml::from_str("value: row.wait_ns\nscale: 0.000001").expect("scaled object");
    assert_eq!(
        scaled,
        GraphValueSpec::Scaled {
            value: BindingExpr::Path("row.wait_ns".to_string()),
            scale: 0.000001
        }
    );
    assert_eq!(
        serde_json::to_value(&scaled).expect("scaled json"),
        json!({
            "value": "row.wait_ns",
            "scale": 0.000001
        })
    );
}
