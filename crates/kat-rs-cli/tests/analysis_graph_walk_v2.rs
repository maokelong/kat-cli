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
    let predicate: PredicateSpec = serde_yaml::from_str("eq: [row.missing, 1]").expect("predicate");
    let source = json!({});
    let row = json!({});
    let facts = json!({});
    let state = json!({});
    let params = json!({});
    let ctx = eval_ctx(&source, &row, &facts, &state, &params, None);

    assert!(!predicate.matches(&ctx).expect("predicate"));
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
