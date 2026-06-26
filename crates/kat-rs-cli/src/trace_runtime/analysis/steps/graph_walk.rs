use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

use crate::trace_runtime::{
    analysis::context::AnalysisState,
    pack::spec::{ConditionOp, EdgeFactSpec, EdgeProviderSpec, EdgeTargetSpec, GraphWalkStepSpec},
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
    let mut frontier = vec![source];
    let mut visited_edge_keys = visited_edge_keys_from_state(state)?;
    let mut selected_edges = Vec::new();
    let mut decisions = Vec::new();
    let mut evidence = Vec::new();

    for _depth in 0..step.limits.max_depth {
        if frontier.is_empty() {
            break;
        }

        let mut next_frontier = Vec::new();
        for source in &frontier {
            let mut selected_for_node = 0usize;

            for provider in &step.edge_providers {
                if selected_for_node >= step.limits.max_edges_per_node {
                    break;
                }

                let Some((_, rows)) = table_rows
                    .iter()
                    .find(|(table, _)| *table == provider.table.as_str())
                else {
                    continue;
                };

                for row in rows {
                    if selected_for_node >= step.limits.max_edges_per_node {
                        break;
                    }

                    if !provider_matches_source(provider, source, row)
                        || !provider_matches_row(provider, row)
                    {
                        continue;
                    }

                    let edge = edge_for_row(provider, source, row);
                    if !visited_edge_keys.insert(visited_edge_key_for_edge(&edge)?) {
                        continue;
                    }

                    next_frontier.push(edge["target"].clone());
                    selected_edges.push(edge);
                    selected_for_node += 1;
                    decisions.push(json!({
                        "step": step.id,
                        "status": "selected",
                        "edgeType": provider.emit.edge_type,
                        "provider": provider.id,
                    }));
                    let facts = selected_edge_facts(provider, row, table_rows);
                    evidence.push(json!({
                        "evidenceId": format!("ev.{}.{}", step.id, provider.id),
                        "status": "ok",
                        "facts": facts,
                        "tableRefs": evidence_table_refs(provider),
                        "limitations": [],
                    }));
                }
            }
        }

        frontier = next_frontier;
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

    state.set_path("frontier.nodes", Value::Array(frontier))?;
    append_state_values(state, "visitedEdges", selected_edges)?;
    append_state_values(state, "decisions", decisions)?;

    Ok(evidence)
}

fn visited_edge_keys_from_state(state: &AnalysisState) -> Result<BTreeSet<String>> {
    let mut keys = BTreeSet::new();
    let Some(edges) = state.value().get("visitedEdges") else {
        return Ok(keys);
    };
    let Value::Array(edges) = edges else {
        bail!("cannot read graph walk visited edges from non-array `visitedEdges`");
    };

    for edge in edges {
        if let Ok(key) = visited_edge_key_for_edge(edge) {
            keys.insert(key);
        }
    }

    Ok(keys)
}

fn selected_edge_facts(
    provider: &EdgeProviderSpec,
    row: &Value,
    table_rows: &[(&str, Vec<Value>)],
) -> Value {
    let mut facts = Map::new();
    facts.insert(
        "selectedEdgeType".to_string(),
        Value::String(provider.emit.edge_type.clone()),
    );
    facts.insert("provider".to_string(), Value::String(provider.id.clone()));
    facts.insert(
        "matchedTable".to_string(),
        Value::String(provider.table.clone()),
    );

    for (fact_key, fact) in &provider.emit.facts {
        if let Some(value) = configured_fact_value(provider, row, table_rows, fact) {
            facts.insert(fact_key.clone(), value);
        }
    }

    Value::Object(facts)
}

fn evidence_table_refs(provider: &EdgeProviderSpec) -> Vec<String> {
    if provider.emit.evidence.is_empty() {
        vec![provider.table.clone()]
    } else {
        provider.emit.evidence.clone()
    }
}

fn rows_for_table<'a>(table_rows: &'a [(&str, Vec<Value>)], table: &str) -> Option<&'a [Value]> {
    table_rows
        .iter()
        .find(|(name, _)| *name == table)
        .map(|(_, rows)| rows.as_slice())
}

fn configured_fact_value(
    provider: &EdgeProviderSpec,
    provider_row: &Value,
    table_rows: &[(&str, Vec<Value>)],
    fact: &EdgeFactSpec,
) -> Option<Value> {
    let table = fact.table.as_deref().unwrap_or(&provider.table);
    let rows = if table == provider.table {
        rows_for_table(table_rows, table).unwrap_or_else(|| std::slice::from_ref(provider_row))
    } else {
        rows_for_table(table_rows, table)?
    };

    if fact.count {
        return Some(json!(rows.len()));
    }

    let field = fact.field.as_deref()?;
    let row = select_fact_row(rows, fact)?;
    let value = row.get(field)?.clone();
    Some(apply_scale(value, fact.scale))
}

fn select_fact_row<'a>(rows: &'a [Value], fact: &EdgeFactSpec) -> Option<&'a Value> {
    if rows.is_empty() {
        return None;
    }

    if fact.row.where_.is_empty() {
        return rows.first();
    }

    rows.iter()
        .find(|row| {
            fact.row
                .where_
                .iter()
                .all(|(field, condition)| condition_matches(row.get(field), condition))
        })
        .or_else(|| {
            (fact.row.fallback.as_deref() == Some("first"))
                .then(|| rows.first())
                .flatten()
        })
}

fn apply_scale(value: Value, scale: Option<f64>) -> Value {
    let Some(scale) = scale else {
        return value;
    };
    match value.as_f64() {
        Some(number) => json!(number * scale),
        None => value,
    }
}

fn provider_matches_row(provider: &EdgeProviderSpec, row: &Value) -> bool {
    provider
        .when
        .iter()
        .all(|(field, condition)| condition_matches(row.get(field), condition))
}

fn provider_matches_source(provider: &EdgeProviderSpec, source: &Value, row: &Value) -> bool {
    provider.source.iter().all(|(source_field, row_field)| {
        let Some(source_value) = source.get(source_field) else {
            return false;
        };
        let Some(row_value) = row.get(row_field) else {
            return false;
        };

        values_equal(source_value, row_value)
    })
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
        "score": score_for_row(provider, row),
    })
}

fn visited_edge_key_for_edge(edge: &Value) -> Result<String> {
    let provider = edge
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let edge_type = edge
        .get("edgeType")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let source = edge.get("source").unwrap_or(&Value::Null);
    let target = edge.get("target").unwrap_or(&Value::Null);

    Ok(serde_json::to_string(&(
        provider, edge_type, source, target,
    ))?)
}

fn score_for_row(provider: &EdgeProviderSpec, row: &Value) -> f64 {
    provider
        .emit
        .score
        .as_deref()
        .and_then(|field| row.get(field))
        .and_then(Value::as_f64)
        .unwrap_or(1.0)
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
