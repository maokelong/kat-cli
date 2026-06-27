use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::trace_runtime::analysis::{
    context::AnalysisState,
    graph::{
        GraphCandidate,
        binding::EvalContext,
        evidence::{candidate_decision, candidate_edge, candidate_evidence},
        expand::{expand_node, output_annotations},
        select::select_candidates,
        spec::{GenericGraphWalkStepSpec, GraphProviderSpec},
    },
};

const NO_EDGE_REASON: &str = "No graph edge provider matched current rows";

pub fn run_graph_walk_on_rows_v2(
    step: &GenericGraphWalkStepSpec,
    state: &mut AnalysisState,
    params: &Value,
    table_rows: &[(&str, Vec<Value>)],
) -> Result<Vec<Value>> {
    let root = value_at_path(state.value(), &step.root.from_state, "root.fromState")
        .map_err(|error| {
            anyhow::anyhow!(
                "invalid root fromState path {:?}: {error}",
                step.root.from_state
            )
        })?
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut frontier = match root {
        Value::Array(nodes) => nodes,
        root => vec![root],
    };
    let mut visited_keys = visited_keys_from_state(state)?;
    let mut selected_edges = Vec::new();
    let mut decisions = Vec::new();
    let mut evidence = Vec::new();
    let mut selected_count = 0usize;
    let mut saw_candidate = false;

    for _depth in 0..step.limits.max_depth {
        if frontier.is_empty() || selected_count >= step.limits.max_nodes {
            break;
        }

        let mut next_frontier = Vec::new();
        for source in &frontier {
            if selected_count >= step.limits.max_nodes {
                break;
            }

            let mut selected_for_source = 0usize;
            let empty_facts = json!({});
            let facts = source.get("facts").unwrap_or(&empty_facts);
            for provider in &step.providers {
                if selected_for_source >= step.limits.max_edges_per_node
                    || selected_count >= step.limits.max_nodes
                {
                    break;
                }

                let candidates = candidates_for_provider(
                    provider,
                    source,
                    facts,
                    state.value(),
                    params,
                    table_rows,
                )?;
                if !candidates.is_empty() {
                    saw_candidate = true;
                }
                let selected =
                    select_candidates(candidates, &provider.select, facts, state.value(), params)?;

                for candidate in selected {
                    if selected_for_source >= step.limits.max_edges_per_node
                        || selected_count >= step.limits.max_nodes
                    {
                        break;
                    }
                    if !visited_keys.insert(visited_key_for_candidate(&candidate)?) {
                        continue;
                    }

                    let edge = candidate_edge(&candidate);
                    next_frontier.push(candidate.node.clone());
                    selected_edges.push(edge);
                    decisions.push(candidate_decision(&step.id, &candidate));
                    evidence.push(candidate_evidence(&step.id, evidence.len(), &candidate)?);
                    selected_for_source += 1;
                    selected_count += 1;
                }
            }
        }

        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }

    state.set_path("frontier.nodes", Value::Array(frontier))?;

    if selected_edges.is_empty() && saw_candidate {
        return Ok(evidence);
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
            "evidenceId": format!("{}:no_edge", step.id),
            "status": "partial",
            "facts": {},
            "tableRefs": [],
            "limitations": [NO_EDGE_REASON],
        })]);
    }

    append_state_values(state, "graph.visited", selected_edges.clone())?;
    append_state_values(state, "visitedEdges", selected_edges)?;
    append_state_values(state, "decisions", decisions)?;

    Ok(evidence)
}

fn candidates_for_provider(
    provider: &GraphProviderSpec,
    source: &Value,
    facts: &Value,
    state: &Value,
    params: &Value,
    table_rows: &[(&str, Vec<Value>)],
) -> Result<Vec<GraphCandidate>> {
    let mut candidates = Vec::new();
    for row in rows_for_table(table_rows, &provider.input.table) {
        let match_ctx = EvalContext {
            source,
            row,
            facts,
            state,
            params,
            node: None,
        };
        if !provider.match_.matches(&match_ctx)? {
            continue;
        }

        let node = expand_node(&provider.expand, &match_ctx)?;
        let annotation_ctx = EvalContext {
            source,
            row,
            facts,
            state,
            params,
            node: Some(&node),
        };
        let annotations = output_annotations(&provider.output, &annotation_ctx)?;
        let evidence_tables = if provider.output.evidence.tables.is_empty() {
            vec![provider.input.table.clone()]
        } else {
            provider.output.evidence.tables.clone()
        };

        candidates.push(GraphCandidate {
            provider_id: provider.id.clone(),
            input_table: provider.input.table.clone(),
            relation: provider.output.relation.clone(),
            source: source.clone(),
            row: row.clone(),
            node,
            annotations,
            evidence_tables,
        });
    }

    Ok(candidates)
}

fn rows_for_table<'a>(table_rows: &'a [(&str, Vec<Value>)], table: &str) -> &'a [Value] {
    table_rows
        .iter()
        .find(|(name, _)| *name == table)
        .map(|(_, rows)| rows.as_slice())
        .unwrap_or(&[])
}

fn visited_keys_from_state(state: &AnalysisState) -> Result<BTreeSet<String>> {
    let mut keys = BTreeSet::new();
    for path in ["graph.visited", "visitedEdges"] {
        let Some(edges) = value_at_path(state.value(), path, path)? else {
            continue;
        };
        let Value::Array(edges) = edges else {
            bail!("cannot read graph walk visited edges from non-array `{path}`");
        };
        for edge in edges {
            if let Ok(key) = visited_key_for_edge(edge) {
                keys.insert(key);
            }
        }
    }

    Ok(keys)
}

fn visited_key_for_candidate(candidate: &GraphCandidate) -> Result<String> {
    Ok(serde_json::to_string(&(
        &candidate.provider_id,
        &candidate.relation,
        &candidate.source,
        &candidate.node,
    ))?)
}

fn visited_key_for_edge(edge: &Value) -> Result<String> {
    let provider = edge
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let relation = edge
        .get("relation")
        .or_else(|| edge.get("edgeType"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let source = edge.get("source").unwrap_or(&Value::Null);
    let target = edge.get("target").unwrap_or(&Value::Null);

    Ok(serde_json::to_string(&(
        provider, relation, source, target,
    ))?)
}

fn value_at_path<'a>(value: &'a Value, path: &str, context: &str) -> Result<Option<&'a Value>> {
    if path.is_empty() {
        bail!("empty analysis state path for {context}");
    }

    let mut current = value;
    for segment in path.split('.') {
        if segment.is_empty() {
            bail!("empty analysis state path segment in {context} path {path:?}");
        }
        let Some(next) = current.get(segment) else {
            return Ok(None);
        };
        current = next;
    }
    Ok(Some(current))
}

fn append_state_values(state: &mut AnalysisState, path: &str, values: Vec<Value>) -> Result<()> {
    let mut array = match value_at_path(state.value(), path, path)?
        .cloned()
        .unwrap_or_else(|| json!([]))
    {
        Value::Array(array) => array,
        _ => bail!("cannot append graph walk state to non-array `{path}`"),
    };

    array.extend(values);
    state.set_path(path, Value::Array(array))
}
