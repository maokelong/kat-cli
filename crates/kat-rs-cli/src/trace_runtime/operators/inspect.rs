use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde_json::{Value, json};

use super::time::string_field;

const REQUIRED_TABLES: [&str; 7] = [
    "process",
    "thread",
    "thread_state",
    "instant",
    "sched_slice",
    "callstack",
    "trace_range",
];

pub fn inspect_trace(schema: &str, rows: Vec<Value>) -> Result<Value> {
    let present_tables = rows
        .iter()
        .filter_map(|row| string_field(row, "name"))
        .collect::<BTreeSet<_>>();
    let trace_range = rows
        .iter()
        .find(|row| string_field(row, "name").as_deref() == Some("trace_range"))
        .cloned()
        .unwrap_or(Value::Null);
    let required_table_status = REQUIRED_TABLES
        .iter()
        .map(|table| {
            (
                (*table).to_string(),
                json!({
                    "present": present_tables.contains(*table)
                }),
            )
        })
        .collect::<BTreeMap<_, _>>();

    Ok(json!({
        "schema": schema,
        "status": "ok",
        "facts": {
            "trace_range": trace_range,
            "tables": rows,
            "required_table_status": required_table_status
        },
        "limitations": []
    }))
}
