use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

use crate::trace_runtime::{
    analysis::context::AnalysisState,
    pack::spec::{ConditionOp, EdgeProviderSpec, EdgeTargetSpec, GraphWalkStepSpec},
};

const NO_EDGE_REASON: &str = "No graph edge provider matched current rows";

pub fn run_graph_walk_on_rows(
    step: &GraphWalkStepSpec,
    state: &mut AnalysisState,
    table_rows: &[(&str, Vec<Value>)],
) -> Result<Vec<Value>> {
    let source = state
        .value()
        .get(&step.root.from_state)
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut selected_edges = Vec::new();
    let mut decisions = Vec::new();
    let mut evidence = Vec::new();

    for provider in &step.edge_providers {
        if selected_edges.len() >= step.limits.max_edges_per_node {
            break;
        }

        let Some((_, rows)) = table_rows
            .iter()
            .find(|(table, _)| *table == provider.table.as_str())
        else {
            continue;
        };

        for row in rows {
            if selected_edges.len() >= step.limits.max_edges_per_node {
                break;
            }

            if !provider_matches_row(provider, row) {
                continue;
            }

            selected_edges.push(edge_for_row(provider, &source, row));
            decisions.push(json!({
                "step": step.id,
                "status": "selected",
                "edgeType": provider.emit.edge_type,
                "provider": provider.id,
            }));
            evidence.push(json!({
                "evidenceId": format!("ev.{}.{}", step.id, provider.id),
                "status": "ok",
                "facts": {
                    "selectedEdgeType": provider.emit.edge_type,
                    "provider": provider.id,
                    "matchedTable": provider.table,
                },
                "tableRefs": [provider.table],
                "limitations": [],
            }));
        }
    }

    if selected_edges.is_empty() {
        append_state_values(
            state,
            "decisions",
            vec![json!({
                "step": step.id,
                "status": "no_edge",
                "reason": NO_EDGE_REASON,
            })],
        )?;

        return Ok(vec![json!({
            "evidenceId": format!("ev.{}.no_edge", step.id),
            "status": "partial",
            "facts": {},
            "tableRefs": [],
            "limitations": [NO_EDGE_REASON],
        })]);
    }

    append_state_values(state, "visitedEdges", selected_edges)?;
    append_state_values(state, "decisions", decisions)?;

    Ok(evidence)
}

fn provider_matches_row(provider: &EdgeProviderSpec, row: &Value) -> bool {
    provider
        .when
        .iter()
        .all(|(field, condition)| condition_matches(row.get(field), condition))
}

fn condition_matches(value: Option<&Value>, condition: &ConditionOp) -> bool {
    match condition {
        ConditionOp::Eq(expected) => value
            .map(|actual| values_equal(actual, expected))
            .unwrap_or(false),
        ConditionOp::Neq(expected) => value
            .map(|actual| !values_equal(actual, expected))
            .unwrap_or(false),
        ConditionOp::Gte(expected) => value
            .and_then(Value::as_f64)
            .map(|actual| actual >= *expected)
            .unwrap_or(false),
        ConditionOp::Gt(expected) => value
            .and_then(Value::as_f64)
            .map(|actual| actual > *expected)
            .unwrap_or(false),
        ConditionOp::Lte(expected) => value
            .and_then(Value::as_f64)
            .map(|actual| actual <= *expected)
            .unwrap_or(false),
        ConditionOp::Lt(expected) => value
            .and_then(Value::as_f64)
            .map(|actual| actual < *expected)
            .unwrap_or(false),
        ConditionOp::Exists(expected) => value.is_some() == *expected,
    }
}

fn values_equal(actual: &Value, expected: &Value) -> bool {
    match (actual.as_f64(), expected.as_f64()) {
        (Some(actual), Some(expected)) => (actual - expected).abs() < f64::EPSILON,
        _ => actual == expected,
    }
}

fn edge_for_row(provider: &EdgeProviderSpec, source: &Value, row: &Value) -> Value {
    json!({
        "provider": provider.id,
        "edgeType": provider.emit.edge_type,
        "source": source,
        "target": target_for_row(&provider.emit.target, source, row),
        "evidenceRefs": provider.emit.evidence,
        "score": row
            .get("dominant_percent")
            .and_then(Value::as_f64)
            .unwrap_or(1.0),
    })
}

fn target_for_row(target: &EdgeTargetSpec, source: &Value, row: &Value) -> Value {
    if target.same_node {
        return source.clone();
    }

    let mut target_object = Map::new();
    insert_target_field(&mut target_object, "itid", target.itid.as_deref(), row);
    insert_target_field(
        &mut target_object,
        "start_ts",
        target.start_ts.as_deref(),
        row,
    );
    insert_target_field(&mut target_object, "end_ts", target.end_ts.as_deref(), row);

    Value::Object(target_object)
}

fn insert_target_field(
    target_object: &mut Map<String, Value>,
    target_key: &str,
    row_key: Option<&str>,
    row: &Value,
) {
    let Some(row_key) = row_key else {
        return;
    };
    let Some(value) = row.get(row_key) else {
        return;
    };

    target_object.insert(target_key.to_owned(), value.clone());
}

fn append_state_values(state: &mut AnalysisState, key: &str, values: Vec<Value>) -> Result<()> {
    let mut array = match state.value().get(key).cloned().unwrap_or_else(|| json!([])) {
        Value::Array(array) => array,
        _ => bail!("cannot append graph walk state to non-array `{key}`"),
    };

    array.extend(values);
    state.set_path(key, Value::Array(array))
}
