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
    summarize_thread_state_profile(&mut facts, row);

    for table in &provider.emit.evidence {
        match table.as_str() {
            "thread_state_profile" => {
                if let Some(rows) = rows_for_table(table_rows, table) {
                    if let Some(first_row) = rows.first() {
                        summarize_thread_state_profile(&mut facts, first_row);
                    }
                }
            }
            "callstack_self_time" => {
                if let Some(rows) = rows_for_table(table_rows, table) {
                    summarize_callstack_self_time(&mut facts, rows);
                }
            }
            "io_sample_overlap" => {
                if let Some(rows) = rows_for_table(table_rows, table) {
                    facts.insert("overlapRows".to_string(), json!(rows.len()));
                }
            }
            _ => {}
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

fn summarize_thread_state_profile(facts: &mut Map<String, Value>, row: &Value) {
    if let Some(value) = row.get("dominant_state") {
        facts.insert("dominantState".to_string(), value.clone());
    }
    if let Some(value) = row.get("dominant_percent") {
        facts.insert("dominantPercent".to_string(), value.clone());
    }
}

fn summarize_callstack_self_time(facts: &mut Map<String, Value>, rows: &[Value]) {
    let Some(top_span) = rows
        .iter()
        .find(|row| row.get("exclusive_rank").and_then(Value::as_i64) == Some(1))
        .or_else(|| rows.first())
    else {
        return;
    };

    if let Some(value) = top_span.get("name") {
        facts.insert("topSpanName".to_string(), value.clone());
    }
    if let Some(duration_ns) = top_span.get("exclusive_dur_ns").and_then(Value::as_f64) {
        facts.insert("topSpanDurMs".to_string(), json!(duration_ns / 1_000_000.0));
    }
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
        "score": score_for_row(provider, row),
    })
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
