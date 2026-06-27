use anyhow::Result;
use serde_json::{Map, Value, json};

use super::GraphCandidate;

pub fn candidate_edge(candidate: &GraphCandidate) -> Value {
    json!({
        "provider": candidate.provider_id,
        "relation": candidate.relation,
        "edgeType": candidate.relation,
        "source": candidate.source,
        "target": candidate.node,
        "annotations": candidate.annotations,
        "evidenceRefs": candidate.evidence_tables,
    })
}

pub fn candidate_decision(step_id: &str, candidate: &GraphCandidate) -> Value {
    json!({
        "step": step_id,
        "status": "selected",
        "provider": candidate.provider_id,
        "relation": candidate.relation,
        "edgeType": candidate.relation,
    })
}

pub fn candidate_evidence(
    step_id: &str,
    index: usize,
    candidate: &GraphCandidate,
) -> Result<Value> {
    let mut facts = Map::new();
    facts.insert(
        "provider".to_string(),
        Value::String(candidate.provider_id.clone()),
    );
    facts.insert(
        "relation".to_string(),
        Value::String(candidate.relation.clone()),
    );
    facts.insert(
        "selectedEdgeType".to_string(),
        Value::String(candidate.relation.clone()),
    );
    facts.insert(
        "matchedTable".to_string(),
        Value::String(candidate.input_table.clone()),
    );
    if let Some(annotations) = candidate.annotations.as_object() {
        for (key, value) in annotations {
            facts.insert(key.clone(), value.clone());
        }
    }

    Ok(json!({
        "evidenceId": format!("{step_id}:{index}"),
        "status": "ok",
        "facts": facts,
        "tableRefs": candidate.evidence_tables,
        "limitations": [],
    }))
}
