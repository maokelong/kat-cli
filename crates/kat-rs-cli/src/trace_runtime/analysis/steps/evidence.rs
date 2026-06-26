use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

use crate::trace_runtime::analysis::context::AnalysisState;

const ROOT_KEYS: &[&str] = &[
    "callstack_id",
    "root_callstack_id",
    "itid",
    "tid",
    "ipid",
    "process_name",
    "thread_name",
    "vsync_id",
    "start_ts",
    "end_ts",
    "dur_ns",
];

const FACT_KEYS: &[&str] = &[
    "callstack_id",
    "root_callstack_id",
    "itid",
    "process_name",
    "vsync_id",
    "start_ts",
    "end_ts",
    "dur_ns",
];

pub fn render_seed_evidence(
    step_id: &str,
    table: &str,
    rows: &[Value],
    state: &mut AnalysisState,
) -> Result<Value> {
    let Some(first_row) = rows.first() else {
        bail!("cannot render seed evidence from empty table `{table}`");
    };
    let Some(first_row) = first_row.as_object() else {
        bail!("cannot render seed evidence from non-object row in table `{table}`");
    };

    let mut facts = Map::new();
    for key in FACT_KEYS {
        if let Some(value) = first_row.get(*key) {
            facts.insert((*key).to_owned(), value.clone());
        }
    }
    if facts.is_empty() {
        bail!("cannot render seed evidence from table `{table}` without facts");
    }

    let mut staged_root = match state.value().get("root") {
        Some(root) if root.is_object() => root.clone(),
        Some(_) => bail!("cannot render seed evidence into non-object root state"),
        None => json!({}),
    };
    let staged_root_object = staged_root
        .as_object_mut()
        .expect("staged root was initialized as an object");
    for key in ROOT_KEYS {
        if let Some(value) = first_row.get(*key) {
            staged_root_object.insert((*key).to_owned(), value.clone());
        }
    }

    if let Some(frontier) = state.value().get("frontier") {
        if !frontier.is_object() {
            bail!("cannot render seed evidence into non-object frontier state");
        }
    }

    state.set_path("root", staged_root.clone())?;
    state.set_path("frontier.nodes", json!([staged_root]))?;

    Ok(json!({
        "evidenceId": format!("ev.{step_id}.{table}"),
        "status": "ok",
        "facts": facts,
        "tableRefs": [table],
        "limitations": [],
    }))
}
