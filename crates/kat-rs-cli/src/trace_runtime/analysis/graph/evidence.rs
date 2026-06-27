use anyhow::Result;
use serde_json::{Value, json};

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
    Ok(json!({
        "evidenceId": format!("{step_id}:{index}"),
        "status": "ok",
        "facts": {
            "provider": candidate.provider_id,
            "relation": candidate.relation,
            "selectedEdgeType": candidate.relation,
            "matchedTable": candidate.input_table,
            "annotations": candidate.annotations,
        },
        "tableRefs": candidate.evidence_tables,
        "limitations": [],
    }))
}
