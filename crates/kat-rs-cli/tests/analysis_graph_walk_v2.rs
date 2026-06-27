use kat_rs_cli::trace_runtime::analysis::graph::binding::{BindingExpr, EvalContext};
use serde_json::json;

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
