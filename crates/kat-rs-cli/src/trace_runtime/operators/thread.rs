use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde_json::{Value, json};

use super::time::{bool_field, i64_field, string_field};

pub fn resolve_thread_candidates(schema: &str, rows: Vec<Value>) -> Result<Value> {
    let status = if rows.is_empty() {
        "empty_result"
    } else {
        "ok"
    };

    Ok(json!({
        "schema": schema,
        "status": status,
        "facts": {
            "candidates": rows
        },
        "limitations": []
    }))
}

pub fn classify_thread_identity(schema: &str, params: Value, rows: Vec<Value>) -> Result<Value> {
    let requested_itids = integer_array(&params, "itids")
        .or_else(|| integer_array(&params, "utids"))
        .unwrap_or_default();
    let seen_itids = rows
        .iter()
        .filter_map(|row| i64_field(row, "itid"))
        .collect::<BTreeSet<_>>();
    let missing_itids = requested_itids
        .iter()
        .copied()
        .filter(|itid| !seen_itids.contains(itid))
        .collect::<Vec<_>>();

    let threads = rows
        .into_iter()
        .map(|row| {
            let thread_name = string_field(&row, "thread_name")
                .or_else(|| string_field(&row, "name"))
                .unwrap_or_default();
            let is_irq_thread = is_irq_thread(&thread_name);
            let is_io_thread_candidate = is_io_thread_candidate(&thread_name);

            json!({
                "itid": i64_field(&row, "itid"),
                "tid": i64_field(&row, "tid"),
                "thread_name": thread_name,
                "is_main_thread": bool_field(&row, "is_main_thread"),
                "ipid": i64_field(&row, "ipid"),
                "pid": i64_field(&row, "pid"),
                "process_name": string_field(&row, "process_name"),
                "is_irq_thread": is_irq_thread,
                "is_io_thread_candidate": is_io_thread_candidate,
                "classification": classification(is_irq_thread, is_io_thread_candidate)
            })
        })
        .collect::<Vec<_>>();

    let status = if threads.is_empty() {
        "empty_result"
    } else if !missing_itids.is_empty() {
        "partial"
    } else {
        "ok"
    };

    Ok(json!({
        "schema": schema,
        "status": status,
        "facts": {
            "requested_itids": requested_itids,
            "missing_itids": missing_itids,
            "threads": threads,
            "rules": BTreeMap::from([
                ("irq_thread".to_string(), json!("thread_name contains udk-irq")),
                ("io_thread_candidate".to_string(), json!("fsverity/cdecrypt/erofs_unzipd/fsignature/hmfs/wk patterns, excluding hmfs_txn")),
            ])
        },
        "limitations": []
    }))
}

fn integer_array(params: &Value, field: &str) -> Option<Vec<i64>> {
    Some(
        params
            .get(field)?
            .as_array()?
            .iter()
            .filter_map(|value| match value {
                Value::Number(number) => {
                    number.as_i64().or_else(|| number.as_u64()?.try_into().ok())
                }
                Value::String(value) => value.parse().ok(),
                _ => None,
            })
            .collect(),
    )
}

fn is_irq_thread(thread_name: &str) -> bool {
    thread_name.to_ascii_lowercase().contains("udk-irq")
}

fn is_io_thread_candidate(thread_name: &str) -> bool {
    let lower = thread_name.to_ascii_lowercase();
    if lower.contains("hmfs_txn") {
        return false;
    }

    lower.contains("fsverity")
        || lower.contains("cdecrypt")
        || lower.contains("erofs_unzipd")
        || lower.contains("fsignature")
        || lower.contains("hmfs")
        || lower == "wk"
        || lower.starts_with("wk:")
        || lower.starts_with("wk_")
        || lower.starts_with("wk-")
        || lower.contains("/wk")
        || lower.contains("_wk")
        || lower.contains("-wk")
}

fn classification(is_irq_thread: bool, is_io_thread_candidate: bool) -> &'static str {
    if is_irq_thread {
        "irq_thread"
    } else if is_io_thread_candidate {
        "io_thread_candidate"
    } else {
        "generic_thread"
    }
}
