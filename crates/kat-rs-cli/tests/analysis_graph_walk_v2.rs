use kat_rs_cli::trace_runtime::analysis::graph::{
    binding::{BindingExpr, EvalContext},
    predicate::PredicateSpec,
};
use serde_json::{Value, json};

#[test]
fn graph_binding_resolves_literal_path_and_template_values() {
    let source = json!({
        "itid": 405,
        "window": {
            "startTs": 10,
            "endTs": 30
        }
    });
    let row = json!({
        "target_itid": 405,
        "waker_itid": 406,
        "label": "RenderThread"
    });
    let facts = json!({
        "threadState": {
            "dominantState": "S"
        }
    });
    let state = json!({
        "root": {
            "process_name": ".tencent.wechat"
        }
    });
    let params = json!({
        "marker": "firstDrawFrame:1"
    });
    let node = json!({
        "itid": 406
    });
    let ctx = EvalContext {
        source: &source,
        row: &row,
        facts: &facts,
        state: &state,
        params: &params,
        node: Some(&node),
    };

    assert_eq!(
        BindingExpr::Path("source.itid".to_string())
            .resolve(&ctx)
            .expect("source path"),
        Some(json!(405))
    );
    assert_eq!(
        BindingExpr::Path("facts.threadState.dominantState".to_string())
            .resolve(&ctx)
            .expect("facts path"),
        Some(json!("S"))
    );
    assert_eq!(
        BindingExpr::Template("${row.label}:${params.marker}".to_string())
            .resolve(&ctx)
            .expect("template"),
        Some(json!("RenderThread:firstDrawFrame:1"))
    );
    assert_eq!(
        BindingExpr::Template("${source.itid}".to_string())
            .resolve(&ctx)
            .expect("exact template"),
        Some(json!(405))
    );
    assert_eq!(
        BindingExpr::Literal(json!("plain"))
            .resolve(&ctx)
            .expect("literal"),
        Some(json!("plain"))
    );
    assert_eq!(
        BindingExpr::Path("row.missing".to_string())
            .resolve(&ctx)
            .expect("missing path"),
        None
    );
}

#[test]
fn graph_binding_deserializes_paths_templates_and_literals() {
    assert_eq!(
        serde_yaml::from_str::<BindingExpr>("row.wake_ts").expect("path"),
        BindingExpr::Path("row.wake_ts".to_string())
    );
    assert_eq!(
        serde_yaml::from_str::<BindingExpr>("source").expect("bare source"),
        BindingExpr::Path("source".to_string())
    );
    assert_eq!(
        serde_yaml::from_str::<BindingExpr>("'${row.label}'").expect("template"),
        BindingExpr::Template("${row.label}".to_string())
    );
    assert_eq!(
        serde_yaml::from_str::<BindingExpr>("'row.${field}'")
            .expect("template with path-like prefix"),
        BindingExpr::Template("row.${field}".to_string())
    );
    assert_eq!(
        serde_yaml::from_str::<BindingExpr>("plain_label").expect("literal string"),
        BindingExpr::Literal(json!("plain_label"))
    );
    assert_eq!(
        serde_yaml::from_str::<BindingExpr>("literal: row.label")
            .expect("explicit path-looking literal"),
        BindingExpr::Literal(json!("row.label"))
    );
    assert_eq!(
        serde_yaml::from_str::<BindingExpr>("literal: '${x}'")
            .expect("explicit template-looking literal"),
        BindingExpr::Literal(json!("${x}"))
    );
    assert_eq!(
        serde_yaml::from_str::<BindingExpr>("literal:\n  value: x")
            .expect("explicit object literal"),
        BindingExpr::Literal(json!({ "value": "x" }))
    );
    assert!(
        serde_yaml::from_str::<BindingExpr>("literal: row.label\nextra: true")
            .expect_err("explicit literal cannot have extra keys")
            .to_string()
            .contains("literal")
    );
    assert_eq!(
        serde_yaml::from_str::<BindingExpr>("42").expect("literal number"),
        BindingExpr::Literal(json!(42))
    );
}

#[test]
fn graph_binding_round_trips_serialized_shapes() {
    fn round_trip(expr: BindingExpr) -> BindingExpr {
        let value = serde_json::to_value(&expr).expect("serialize binding expr");
        serde_json::from_value(value).expect("deserialize binding expr")
    }

    assert_eq!(
        round_trip(BindingExpr::Path("row.label".to_string())),
        BindingExpr::Path("row.label".to_string())
    );
    assert_eq!(
        round_trip(BindingExpr::Template("${row.label}".to_string())),
        BindingExpr::Template("${row.label}".to_string())
    );
    assert_eq!(
        round_trip(BindingExpr::Literal(json!("plain"))),
        BindingExpr::Literal(json!("plain"))
    );
    assert_eq!(
        round_trip(BindingExpr::Literal(json!("row.label"))),
        BindingExpr::Literal(json!("row.label"))
    );
    assert_eq!(
        round_trip(BindingExpr::Literal(json!("${x}"))),
        BindingExpr::Literal(json!("${x}"))
    );
    assert_eq!(
        round_trip(BindingExpr::Literal(json!(42))),
        BindingExpr::Literal(json!(42))
    );
    assert_eq!(
        round_trip(BindingExpr::Literal(json!({ "value": "x" }))),
        BindingExpr::Literal(json!({ "value": "x" }))
    );
    assert_eq!(
        round_trip(BindingExpr::Literal(json!({ "literal": true }))),
        BindingExpr::Literal(json!({ "literal": true }))
    );
    assert_eq!(
        round_trip(BindingExpr::Literal(json!({
            "literal": true,
            "label": "x"
        }))),
        BindingExpr::Literal(json!({
            "literal": true,
            "label": "x"
        }))
    );
}

#[test]
fn graph_binding_value_spec_round_trips_literal_scaled_shape() {
    use kat_rs_cli::trace_runtime::pack::spec::GraphValueSpec;

    let value = GraphValueSpec::Value(BindingExpr::Literal(json!({
        "value": "x",
        "scale": 1.0
    })));
    let serialized = serde_json::to_value(&value).expect("serialize graph value");
    let round_tripped: GraphValueSpec =
        serde_json::from_value(serialized).expect("deserialize graph value");

    assert_eq!(round_tripped, value);
}

#[test]
fn graph_binding_value_spec_falls_back_to_explicit_literal() {
    use kat_rs_cli::trace_runtime::pack::spec::GraphValueSpec;

    let value: GraphValueSpec =
        serde_yaml::from_str("literal:\n  value: x").expect("graph value explicit literal");

    assert_eq!(
        value,
        GraphValueSpec::Value(BindingExpr::Literal(json!({ "value": "x" })))
    );
}

#[test]
fn graph_expand_builds_nested_node_annotations_and_same_as_fallback() {
    use kat_rs_cli::trace_runtime::analysis::graph::{
        expand::{expand_node, output_annotations},
        spec::GraphProviderSpec,
    };

    let provider_yaml = r#"
id: wakeup
input:
  table: wakeup_edges
match:
  exists: row.waker_itid
expand:
  node:
    fields:
      kind: thread_window
      itid: row.waker_itid
      window.startTs: row.waker_start_ts
      window.endTs: row.wake_ts
      missing: row.missing
output:
  relation: wakeup
  annotations:
    wakeTs: row.wake_ts
    waitMs:
      value: row.wait_ns
      scale: 0.000001
"#;
    let provider: GraphProviderSpec = serde_yaml::from_str(provider_yaml).expect("provider");
    let source = json!({ "itid": 405 });
    let row = json!({
        "waker_itid": 406,
        "waker_start_ts": 10,
        "wake_ts": 20,
        "wait_ns": 2_500_000
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    let node = expand_node(&provider.expand, &ctx).expect("expanded node");
    assert_eq!(
        node,
        json!({
            "kind": "thread_window",
            "itid": 406,
            "window": {
                "startTs": 10,
                "endTs": 20
            }
        })
    );

    let annotations = output_annotations(&provider.output, &ctx).expect("annotations");
    assert_eq!(
        annotations,
        json!({
            "wakeTs": 20,
            "waitMs": 2.5
        })
    );

    let same_as_provider: GraphProviderSpec = serde_yaml::from_str(
        r#"
id: direct
input:
  table: wakeup_edges
match:
  exists: row.waker_itid
expand:
  node:
    sameAs: row.same_node
    fields:
      fallback: row.waker_itid
output:
  relation: wakeup
"#,
    )
    .expect("sameAs provider");
    let row_with_same_as = json!({
        "same_node": {
            "kind": "thread",
            "itid": 777
        },
        "waker_itid": 406
    });
    let ctx_with_same_as = eval_ctx(&source, &row_with_same_as, &facts, &state, &params, None);
    assert_eq!(
        expand_node(&same_as_provider.expand, &ctx_with_same_as).expect("sameAs node"),
        json!({
            "kind": "thread",
            "itid": 777
        })
    );

    let row_without_same_as = json!({ "waker_itid": 406 });
    let ctx_without_same_as =
        eval_ctx(&source, &row_without_same_as, &facts, &state, &params, None);
    assert_eq!(
        expand_node(&same_as_provider.expand, &ctx_without_same_as).expect("fallback node"),
        json!({ "fallback": 406 })
    );
}

#[test]
fn graph_expand_selects_ranked_deduped_limited_candidates_with_missing_sort_values() {
    use kat_rs_cli::trace_runtime::analysis::graph::{
        select::select_candidates, spec::GraphSelectSpec,
    };

    let select: GraphSelectSpec = serde_yaml::from_str(
        r#"
limit: 2
orderBy:
  - expr: row.wait_ns
    desc: true
  - expr: row.label
dedupeBy:
  - node.itid
"#,
    )
    .expect("select spec");
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let candidates = vec![
        graph_candidate(
            json!({ "rank": 0 }),
            json!({ "wait_ns": 10, "label": "b" }),
            json!({ "itid": 1 }),
        ),
        graph_candidate(
            json!({ "rank": 1 }),
            json!({ "wait_ns": 30, "label": "a" }),
            json!({ "itid": 1 }),
        ),
        graph_candidate(
            json!({ "rank": 2 }),
            json!({ "wait_ns": 20, "label": "c" }),
            json!({ "itid": 2 }),
        ),
        graph_candidate(
            json!({ "rank": 3 }),
            json!({ "label": "d" }),
            json!({ "itid": 3 }),
        ),
    ];

    let selected =
        select_candidates(candidates, &select, &facts, &state, &params).expect("selected");

    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].row["wait_ns"], json!(30));
    assert_eq!(selected[0].node["itid"], json!(1));
    assert_eq!(selected[1].row["wait_ns"], json!(20));
    assert_eq!(selected[1].node["itid"], json!(2));

    let asc_select: GraphSelectSpec = serde_yaml::from_str(
        r#"
orderBy:
  - expr: row.wait_ns
"#,
    )
    .expect("ascending select");
    let asc_candidates = vec![
        graph_candidate(
            json!({ "rank": 0 }),
            json!({ "wait_ns": 10 }),
            json!({ "itid": 1 }),
        ),
        graph_candidate(json!({ "rank": 1 }), json!({}), json!({ "itid": 2 })),
        graph_candidate(
            json!({ "rank": 2 }),
            json!({ "wait_ns": 5 }),
            json!({ "itid": 3 }),
        ),
    ];
    let asc_selected = select_candidates(asc_candidates, &asc_select, &facts, &state, &params)
        .expect("ascending selected");

    assert_eq!(
        asc_selected
            .iter()
            .map(|candidate| candidate.source["rank"].clone())
            .collect::<Vec<_>>(),
        vec![json!(2), json!(0), json!(1)]
    );
}

#[test]
fn graph_expand_candidate_evidence_helpers_emit_expected_json_shape() {
    use kat_rs_cli::trace_runtime::analysis::graph::{
        GraphCandidate,
        evidence::{candidate_decision, candidate_edge, candidate_evidence},
    };

    let candidate = GraphCandidate {
        provider_id: "wakeup".to_string(),
        input_table: "wakeup_edges".to_string(),
        relation: "wakeup".to_string(),
        source: json!({ "itid": 405 }),
        row: json!({ "wake_ts": 20 }),
        node: json!({ "itid": 406 }),
        annotations: json!({
            "wakeTs": 20,
            "waitMs": 2.5
        }),
        evidence_tables: vec!["wakeup_edges".to_string(), "thread_state".to_string()],
    };

    let edge = candidate_edge(&candidate);
    assert_eq!(edge["provider"], json!("wakeup"));
    assert_eq!(edge["relation"], json!("wakeup"));
    assert_eq!(edge["edgeType"], json!("wakeup"));
    assert_eq!(edge["source"], json!({ "itid": 405 }));
    assert_eq!(edge["target"], json!({ "itid": 406 }));
    assert_eq!(edge["annotations"]["waitMs"], json!(2.5));
    assert_eq!(
        edge["evidenceRefs"],
        json!(["wakeup_edges", "thread_state"])
    );

    let decision = candidate_decision("step-1", &candidate);
    assert_eq!(decision["step"], json!("step-1"));
    assert_eq!(decision["status"], json!("selected"));
    assert_eq!(decision["provider"], json!("wakeup"));
    assert_eq!(decision["relation"], json!("wakeup"));
    assert_eq!(decision["edgeType"], json!("wakeup"));

    let evidence = candidate_evidence("step-1", 3, &candidate).expect("evidence");
    assert_eq!(evidence["evidenceId"], json!("step-1:3"));
    assert_eq!(evidence["facts"]["provider"], json!("wakeup"));
    assert_eq!(evidence["facts"]["relation"], json!("wakeup"));
    assert_eq!(evidence["facts"]["selectedEdgeType"], json!("wakeup"));
    assert_eq!(evidence["facts"]["matchedTable"], json!("wakeup_edges"));
    assert_eq!(evidence["facts"]["wakeTs"], json!(20));
    assert!(
        !evidence["facts"]
            .as_object()
            .expect("facts object")
            .contains_key("annotations")
    );
    assert_eq!(
        evidence["tableRefs"],
        json!(["wakeup_edges", "thread_state"])
    );
}

#[test]
fn graph_binding_handles_missing_node_errors_inline_rendering_and_serialization() {
    let source = json!({ "itid": 405 });
    let row = json!({
        "label": "RenderThread",
        "wake_ts": 123,
        "ready": true
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = EvalContext {
        source: &source,
        row: &row,
        facts: &facts,
        state: &state,
        params: &params,
        node: None,
    };

    assert_eq!(
        BindingExpr::Path("node.itid".to_string())
            .resolve(&ctx)
            .expect("missing node"),
        None
    );
    assert!(
        BindingExpr::Path("unknown.itid".to_string())
            .resolve(&ctx)
            .expect_err("unknown root")
            .to_string()
            .contains("unknown binding path root")
    );
    assert!(
        BindingExpr::Path("row..label".to_string())
            .resolve(&ctx)
            .expect_err("empty segment")
            .to_string()
            .contains("empty binding path segment")
    );
    assert_eq!(
        BindingExpr::Template(
            "${row.label}:${row.missing}:${row.wake_ts}:${row.ready}".to_string()
        )
        .resolve(&ctx)
        .expect("inline template"),
        Some(json!("RenderThread::123:true"))
    );

    assert_eq!(
        serde_json::to_value(BindingExpr::Path("row.label".to_string())).expect("path json"),
        json!("row.label")
    );
    assert_eq!(
        serde_json::to_value(BindingExpr::Template("${row.label}".to_string()))
            .expect("template json"),
        json!("${row.label}")
    );
    assert_eq!(
        serde_json::to_value(BindingExpr::Literal(json!({ "plain": true }))).expect("literal json"),
        json!({ "plain": true })
    );
}

#[test]
fn graph_binding_rejects_malformed_templates() {
    let source = json!({ "itid": 405 });
    let row = json!({ "label": "RenderThread" });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = EvalContext {
        source: &source,
        row: &row,
        facts: &facts,
        state: &state,
        params: &params,
        node: None,
    };

    for template in ["${row.label}}", "${row.label", "${}", "${   }"] {
        assert!(
            BindingExpr::Template(template.to_string())
                .resolve(&ctx)
                .is_err(),
            "expected malformed template to fail: {template}"
        );
    }
}

#[test]
fn graph_predicate_evaluates_boolean_and_temporal_conditions() {
    let yaml = r#"
all:
  - eq: [source.itid, row.target_itid]
  - gte: [row.dominant_percent, 50]
  - exists: row.wake_ts
  - temporal.pointWithin:
      point: row.wake_ts
      window:
        start: source.start_ts
        end: source.end_ts
"#;
    let predicate: PredicateSpec = serde_yaml::from_str(yaml).expect("predicate");
    let source = json!({
        "itid": 405,
        "start_ts": 100,
        "end_ts": 200
    });
    let row = json!({
        "target_itid": 405,
        "dominant_percent": 75.0,
        "wake_ts": 150
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    assert!(predicate.matches(&ctx).expect("predicate"));
}

#[test]
fn graph_predicate_treats_missing_match_fields_as_false() {
    let source = json!({});
    let row = json!({});
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    for yaml in [
        "eq: [row.missing, 1]",
        "neq: [row.missing, 1]",
        "exists: row.missing",
    ] {
        let predicate: PredicateSpec = serde_yaml::from_str(yaml).expect("predicate");
        assert!(
            !predicate.matches(&ctx).expect("predicate"),
            "expected predicate to be false: {yaml}"
        );
    }
}

#[test]
fn graph_predicate_evaluates_any_not_and_comparison_operators() {
    let source = json!({});
    let row = json!({
        "state": "Sleeping",
        "wait_ms": 42.0,
        "rank": 3,
        "cap": 42
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    let predicate: PredicateSpec = serde_yaml::from_str(
        r#"
all:
  - any:
      - eq: [row.missing, ready]
      - neq: [row.state, Running]
  - not:
      eq: [row.state, Running]
  - gt: [row.wait_ms, 41]
  - lt: [row.rank, 4]
  - lte: [row.cap, 42.0]
"#,
    )
    .expect("predicate");

    assert!(predicate.matches(&ctx).expect("predicate"));

    let empty_any: PredicateSpec = serde_yaml::from_str("any: []").expect("empty any");
    assert!(!empty_any.matches(&ctx).expect("empty any"));

    let empty_all: PredicateSpec = serde_yaml::from_str("all: []").expect("empty all");
    assert!(empty_all.matches(&ctx).expect("empty all"));
}

#[test]
fn graph_predicate_numeric_comparisons_treat_missing_and_non_numeric_as_false() {
    let source = json!({});
    let row = json!({
        "label": "Sleeping",
        "rank": 3
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    for yaml in [
        "gt: [row.missing, 1]",
        "gte: [row.label, 1]",
        "lt: [row.missing, 1]",
        "lte: [row.label, 1]",
    ] {
        let predicate: PredicateSpec = serde_yaml::from_str(yaml).expect("predicate");
        assert!(
            !predicate.matches(&ctx).expect("predicate"),
            "expected predicate to be false: {yaml}"
        );
    }
}

#[test]
fn graph_predicate_exists_is_false_for_null() {
    let predicate: PredicateSpec = serde_yaml::from_str("exists: row.value").expect("predicate");
    let source = json!({});
    let row = json!({ "value": null });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    assert!(!predicate.matches(&ctx).expect("predicate"));
}

#[test]
fn graph_predicate_evaluates_temporal_overlaps_boundaries() {
    let source = json!({
        "start_ts": 10,
        "end_ts": 20,
        "touching_start": 20,
        "touching_end": 30
    });
    let row = json!({
        "start_ts": 15,
        "end_ts": 25
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    let overlapping: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.overlaps:
  left:
    start: source.start_ts
    end: source.end_ts
  right:
    start: row.start_ts
    end: row.end_ts
"#,
    )
    .expect("overlapping predicate");
    assert!(overlapping.matches(&ctx).expect("overlap"));

    let touching: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.overlaps:
  left:
    start: source.start_ts
    end: source.end_ts
  right:
    start: source.touching_start
    end: source.touching_end
"#,
    )
    .expect("touching predicate");
    assert!(!touching.matches(&ctx).expect("touching"));
}

#[test]
fn graph_predicate_temporal_conditions_treat_missing_and_non_numeric_as_false() {
    let source = json!({
        "start_ts": 10,
        "end_ts": 20,
        "label": "not-a-timestamp"
    });
    let row = json!({
        "point": 15,
        "start_ts": 12,
        "end_ts": 18,
        "label": "also-not-a-timestamp"
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    for yaml in [
        r#"
temporal.pointWithin:
  point: row.missing
  window:
    start: source.start_ts
    end: source.end_ts
"#,
        r#"
temporal.pointWithin:
  point: row.label
  window:
    start: source.start_ts
    end: source.end_ts
"#,
        r#"
temporal.overlaps:
  left:
    start: source.missing
    end: source.end_ts
  right:
    start: row.start_ts
    end: row.end_ts
"#,
        r#"
temporal.overlaps:
  left:
    start: source.start_ts
    end: source.end_ts
  right:
    start: row.start_ts
    end: row.label
"#,
    ] {
        let predicate: PredicateSpec = serde_yaml::from_str(yaml).expect("predicate");
        assert!(
            !predicate.matches(&ctx).expect("predicate"),
            "expected temporal predicate to be false: {yaml}"
        );
    }
}

#[test]
fn graph_predicate_rejects_invalid_temporal_windows() {
    let predicate: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.pointWithin:
  point: row.wake_ts
  window:
    start: source.start_ts
    end: source.end_ts
"#,
    )
    .expect("predicate");
    let source = json!({
        "start_ts": 200,
        "end_ts": 100
    });
    let row = json!({ "wake_ts": 150 });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    assert!(
        predicate
            .matches(&ctx)
            .expect_err("invalid window")
            .to_string()
            .contains("temporal.pointWithin window")
    );
}

#[test]
fn graph_predicate_validates_temporal_point_window_before_point() {
    let predicate: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.pointWithin:
  point: row.missing
  window:
    start: source.start_ts
    end: source.end_ts
"#,
    )
    .expect("predicate");
    let source = json!({
        "start_ts": 200,
        "end_ts": 100
    });
    let row = json!({});
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    let error = predicate
        .matches(&ctx)
        .expect_err("invalid window should not be masked by missing point")
        .to_string();
    assert!(error.contains("temporal.pointWithin"), "{error}");
}

#[test]
fn graph_predicate_rejects_invalid_temporal_overlap_windows() {
    let source = json!({
        "left_start": 20,
        "left_end": 10,
        "right_start": 40,
        "right_end": 30,
        "valid_start": 10,
        "valid_end": 20
    });
    let row = json!({});
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    let invalid_left: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.overlaps:
  left:
    start: source.left_start
    end: source.left_end
  right:
    start: source.valid_start
    end: source.valid_end
"#,
    )
    .expect("invalid left predicate");
    let left_error = invalid_left
        .matches(&ctx)
        .expect_err("invalid left window")
        .to_string();
    assert!(left_error.contains("temporal.overlaps"), "{left_error}");
    assert!(left_error.contains("left"), "{left_error}");

    let invalid_right: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.overlaps:
  left:
    start: source.valid_start
    end: source.valid_end
  right:
    start: source.right_start
    end: source.right_end
"#,
    )
    .expect("invalid right predicate");
    let right_error = invalid_right
        .matches(&ctx)
        .expect_err("invalid right window")
        .to_string();
    assert!(right_error.contains("temporal.overlaps"), "{right_error}");
    assert!(right_error.contains("right"), "{right_error}");
}

#[test]
fn graph_predicate_validates_temporal_overlap_windows_before_later_missing_values() {
    let source = json!({
        "left_start": 20,
        "left_end": 10,
        "right_start": 40,
        "right_end": 30,
        "valid_start": 10,
        "valid_end": 20,
        "label": "not-a-timestamp"
    });
    let row = json!({});
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    let invalid_right: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.overlaps:
  left:
    start: source.valid_start
    end: source.valid_end
  right:
    start: source.right_start
    end: source.right_end
"#,
    )
    .expect("invalid right predicate");
    let right_error = invalid_right
        .matches(&ctx)
        .expect_err("invalid right window")
        .to_string();
    assert!(right_error.contains("temporal.overlaps"), "{right_error}");
    assert!(right_error.contains("right"), "{right_error}");

    let missing_left_invalid_right: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.overlaps:
  left:
    start: row.missing_start
    end: row.missing_end
  right:
    start: source.right_start
    end: source.right_end
"#,
    )
    .expect("missing left invalid right predicate");
    let missing_left_right_error = missing_left_invalid_right
        .matches(&ctx)
        .expect_err("invalid right window should not be masked by missing left")
        .to_string();
    assert!(
        missing_left_right_error.contains("temporal.overlaps"),
        "{missing_left_right_error}"
    );
    assert!(
        missing_left_right_error.contains("right"),
        "{missing_left_right_error}"
    );

    let non_numeric_left_invalid_right: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.overlaps:
  left:
    start: source.label
    end: source.valid_end
  right:
    start: source.right_start
    end: source.right_end
"#,
    )
    .expect("non-numeric left invalid right predicate");
    let non_numeric_left_right_error = non_numeric_left_invalid_right
        .matches(&ctx)
        .expect_err("invalid right window should not be masked by non-numeric left")
        .to_string();
    assert!(
        non_numeric_left_right_error.contains("temporal.overlaps"),
        "{non_numeric_left_right_error}"
    );
    assert!(
        non_numeric_left_right_error.contains("right"),
        "{non_numeric_left_right_error}"
    );

    let invalid_left_missing_right: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.overlaps:
  left:
    start: source.left_start
    end: source.left_end
  right:
    start: row.missing_start
    end: row.missing_end
"#,
    )
    .expect("invalid left missing right predicate");
    let left_error = invalid_left_missing_right
        .matches(&ctx)
        .expect_err("invalid left window should not be masked by missing right")
        .to_string();
    assert!(left_error.contains("temporal.overlaps"), "{left_error}");
    assert!(left_error.contains("left"), "{left_error}");
}

#[test]
fn graph_predicate_preserves_large_integer_equality_precision() {
    let source = json!({});
    let row = json!({
        "left": 9007199254740992_u64,
        "right": 9007199254740993_u64
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    let eq: PredicateSpec = serde_yaml::from_str("eq: [row.left, row.right]").expect("eq");
    let neq: PredicateSpec = serde_yaml::from_str("neq: [row.left, row.right]").expect("neq");

    assert!(!eq.matches(&ctx).expect("eq"));
    assert!(neq.matches(&ctx).expect("neq"));
}

#[test]
fn graph_predicate_compares_float_equality_exactly() {
    let source = json!({});
    let row = json!({
        "left": 0.0,
        "right": f64::MIN_POSITIVE
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    let eq: PredicateSpec = serde_yaml::from_str("eq: [row.left, row.right]").expect("eq");
    let neq: PredicateSpec = serde_yaml::from_str("neq: [row.left, row.right]").expect("neq");

    assert!(!eq.matches(&ctx).expect("eq"));
    assert!(neq.matches(&ctx).expect("neq"));
}

#[test]
fn graph_predicate_preserves_large_integer_ordering_precision() {
    let source = json!({});
    let row = json!({
        "low": 9007199254740992_u64,
        "high": 9007199254740993_u64
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    let gt: PredicateSpec = serde_yaml::from_str("gt: [row.high, row.low]").expect("gt");
    let lte_false: PredicateSpec =
        serde_yaml::from_str("lte: [row.high, row.low]").expect("lte false");
    let lte_true: PredicateSpec =
        serde_yaml::from_str("lte: [row.low, row.high]").expect("lte true");

    assert!(gt.matches(&ctx).expect("gt"));
    assert!(!lte_false.matches(&ctx).expect("lte false"));
    assert!(lte_true.matches(&ctx).expect("lte true"));
}

#[test]
fn graph_predicate_compares_mixed_large_integer_and_float_safely() {
    let source = json!({
        "start_float": 9007199254740992.0,
        "start_after_point": 9007199254740994_u64,
        "end_int": 9007199254740994_u64,
        "end_float": 9007199254740995.0
    });
    let row = json!({
        "big_int": 9007199254740993_u64,
        "big_float": 9007199254740992.0
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    let eq: PredicateSpec = serde_yaml::from_str("eq: [row.big_int, row.big_float]").expect("eq");
    let neq: PredicateSpec =
        serde_yaml::from_str("neq: [row.big_int, row.big_float]").expect("neq");
    let gt: PredicateSpec = serde_yaml::from_str("gt: [row.big_int, row.big_float]").expect("gt");
    let lt: PredicateSpec = serde_yaml::from_str("lt: [row.big_float, row.big_int]").expect("lt");

    assert!(!eq.matches(&ctx).expect("eq"));
    assert!(neq.matches(&ctx).expect("neq"));
    assert!(gt.matches(&ctx).expect("gt"));
    assert!(lt.matches(&ctx).expect("lt"));

    let within: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.pointWithin:
  point: row.big_int
  window:
    start: source.start_float
    end: source.end_int
"#,
    )
    .expect("within predicate");
    assert!(within.matches(&ctx).expect("within"));

    let outside: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.pointWithin:
  point: row.big_int
  window:
    start: source.start_after_point
    end: source.end_float
"#,
    )
    .expect("outside predicate");
    assert!(!outside.matches(&ctx).expect("outside"));
}

#[test]
fn graph_predicate_compares_huge_floats_against_integers_safely() {
    let source = json!({
        "start_ts": 1e40,
        "end_ts": 1
    });
    let row = json!({
        "huge_float": 1e40,
        "negative_huge_float": -1e40,
        "one": 1
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    let gt: PredicateSpec = serde_yaml::from_str("gt: [row.huge_float, row.one]").expect("gt");
    let lt: PredicateSpec = serde_yaml::from_str("lt: [row.one, row.huge_float]").expect("lt");
    let negative_lt: PredicateSpec =
        serde_yaml::from_str("lt: [row.negative_huge_float, row.one]").expect("negative lt");

    assert!(gt.matches(&ctx).expect("gt"));
    assert!(lt.matches(&ctx).expect("lt"));
    assert!(negative_lt.matches(&ctx).expect("negative lt"));

    let invalid_window: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.pointWithin:
  point: row.one
  window:
    start: source.start_ts
    end: source.end_ts
"#,
    )
    .expect("invalid window predicate");
    let error = invalid_window
        .matches(&ctx)
        .expect_err("huge float invalid window")
        .to_string();
    assert!(error.contains("temporal.pointWithin window"), "{error}");
}

#[test]
fn graph_predicate_preserves_large_integer_temporal_point_within_precision() {
    let predicate: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.pointWithin:
  point: row.point
  window:
    start: source.start_ts
    end: source.end_ts
"#,
    )
    .expect("predicate");
    let source = json!({
        "start_ts": 9007199254740992_u64,
        "end_ts": 9007199254740994_u64
    });
    let row = json!({
        "point": 9007199254740993_u64
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    assert!(predicate.matches(&ctx).expect("predicate"));
}

#[test]
fn graph_predicate_preserves_large_integer_temporal_overlap_precision() {
    let source = json!({
        "start_ts": 9007199254740992_u64,
        "end_ts": 9007199254740994_u64,
        "touching_start": 9007199254740994_u64,
        "touching_end": 9007199254740995_u64
    });
    let row = json!({
        "start_ts": 9007199254740993_u64,
        "end_ts": 9007199254740995_u64
    });
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    let overlapping: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.overlaps:
  left:
    start: source.start_ts
    end: source.end_ts
  right:
    start: row.start_ts
    end: row.end_ts
"#,
    )
    .expect("overlapping predicate");
    assert!(overlapping.matches(&ctx).expect("overlap"));

    let touching: PredicateSpec = serde_yaml::from_str(
        r#"
temporal.overlaps:
  left:
    start: source.start_ts
    end: source.end_ts
  right:
    start: source.touching_start
    end: source.touching_end
"#,
    )
    .expect("touching predicate");
    assert!(!touching.matches(&ctx).expect("touching"));
}

#[test]
fn graph_predicate_rejects_ambiguous_predicate_objects() {
    let error = serde_yaml::from_str::<PredicateSpec>(
        r#"
eq: [row.value, 1]
exists: row.value
"#,
    )
    .expect_err("ambiguous predicate should fail");

    assert!(error.to_string().contains("predicate"));
}

fn eval_ctx<'a>(
    source: &'a Value,
    row: &'a Value,
    facts: &'a Value,
    state: &'a Value,
    params: &'a Value,
    node: Option<&'a Value>,
) -> EvalContext<'a> {
    EvalContext {
        source,
        row,
        facts,
        state,
        params,
        node,
    }
}

fn graph_candidate(
    source: Value,
    row: Value,
    node: Value,
) -> kat_rs_cli::trace_runtime::analysis::graph::GraphCandidate {
    kat_rs_cli::trace_runtime::analysis::graph::GraphCandidate {
        provider_id: "wakeup".to_string(),
        input_table: "wakeup_edges".to_string(),
        relation: "wakeup".to_string(),
        source,
        row,
        node,
        annotations: json!({}),
        evidence_tables: vec!["wakeup_edges".to_string()],
    }
}
