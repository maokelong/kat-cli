use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use super::time::{clipped_duration, i64_field, string_field};

struct CallstackSpan {
    id: Option<i64>,
    parent_id: Option<i64>,
    name: String,
    clip_start_ts: i64,
    clip_end_ts: i64,
    overlap_ns: i64,
    value: Value,
}

pub fn profile_thread_state(schema: &str, params: Value, rows: Vec<Value>) -> Result<Value> {
    let start_ts = i64_field(&params, "start_ts").ok_or_else(|| anyhow!("missing start_ts"))?;
    let end_ts = i64_field(&params, "end_ts").ok_or_else(|| anyhow!("missing end_ts"))?;
    let target_itid = i64_field(&params, "itid");
    let depth = i64_field(&params, "depth").unwrap_or_default();
    let max_depth = i64_field(&params, "max_depth").unwrap_or(i64::MAX);
    let max_depth_reached = depth >= max_depth;
    let visited_edges = string_array(&params, "visited_edges");
    let mut visited_edges_after = visited_edges.clone();
    if let Some(selected_edge_id) = selected_edge_id(&params) {
        if !visited_edges_after.contains(&selected_edge_id) {
            visited_edges_after.push(selected_edge_id);
        }
    }

    let mut state_summary_ns = BTreeMap::<String, i64>::new();
    let mut segments = Vec::new();
    let mut candidate_wait_segments = Vec::new();
    let mut new_candidate_edges = Vec::new();
    let mut repeated_candidate_edges = Vec::new();
    let mut candidate_edge_count = 0;
    let mut current_dependency_start = None;
    let mut running_ns = 0;

    for row in rows {
        if let Some(itid) = target_itid {
            if i64_field(&row, "itid") != Some(itid) {
                continue;
            }
        }

        let Some(ts) = i64_field(&row, "ts") else {
            continue;
        };
        let dur = i64_field(&row, "dur").unwrap_or_default();
        let Some((clip_start_ts, clip_end_ts, duration_ns)) =
            clipped_duration(ts, dur, start_ts, end_ts)
        else {
            continue;
        };

        let state = string_field(&row, "state").unwrap_or_else(|| "unknown".to_string());
        let state_class = classify_thread_state(&state);
        *state_summary_ns.entry(state_class.to_string()).or_default() += duration_ns;
        if state_class == "running" {
            running_ns += duration_ns;
            current_dependency_start = None;
        } else if current_dependency_start.is_none() {
            current_dependency_start = Some(clip_start_ts);
        }

        let segment = json!({
            "itid": i64_field(&row, "itid").or(target_itid),
            "tid": i64_field(&row, "tid"),
            "pid": i64_field(&row, "pid"),
            "thread_name": string_field(&row, "thread_name"),
            "process_name": string_field(&row, "process_name"),
            "ts": ts,
            "dur": dur,
            "clip_start_ts": clip_start_ts,
            "clip_end_ts": clip_end_ts,
            "duration_ns": duration_ns,
            "cpu": row.get("cpu").cloned(),
            "state": state,
            "state_class": state_class,
            "io_wait": row.get("io_wait").cloned(),
            "blocked_function": row.get("blocked_function").cloned(),
            "waker_itid": i64_field(&row, "waker_itid"),
            "wakeup_ts": i64_field(&row, "wakeup_ts")
        });

        if state_class != "running" {
            candidate_wait_segments.push(segment.clone());
        }

        if state_class != "running" {
            if let (Some(blocked_itid), Some(waker_itid), Some(wakeup_ts)) = (
                i64_field(&row, "itid").or(target_itid),
                i64_field(&row, "waker_itid"),
                i64_field(&row, "wakeup_ts"),
            ) {
                let dependency_start = current_dependency_start.unwrap_or(start_ts);
                let edge_id =
                    format!("edge.{blocked_itid}.{waker_itid}.{wakeup_ts}.{dependency_start}");
                candidate_edge_count += 1;
                if visited_edges.contains(&edge_id) {
                    repeated_candidate_edges.push(edge_id);
                } else if !max_depth_reached {
                    new_candidate_edges.push(json!({
                        "edge_id": edge_id,
                        "status": "pending",
                        "blocked_itid": blocked_itid,
                        "waker_itid": waker_itid,
                        "wakeup_ts": wakeup_ts,
                        "dependency_start_ts": dependency_start,
                        "dependency_end_ts": wakeup_ts.min(clip_end_ts),
                        "blocked_state": segment["state"].clone(),
                        "blocked_state_class": segment["state_class"].clone(),
                        "depth": depth + 1
                    }));
                }
            }
        }

        segments.push(segment);
    }

    let mut next_candidate_edges = object_array(&params, "candidate_frontier");
    for edge in &new_candidate_edges {
        let edge_id = edge.get("edge_id").and_then(Value::as_str);
        let already_present = next_candidate_edges
            .iter()
            .any(|candidate| candidate.get("edge_id").and_then(Value::as_str) == edge_id);
        if !already_present {
            next_candidate_edges.push(edge.clone());
        }
    }

    let window_duration = (end_ts - start_ts).max(0);
    let status = if segments.is_empty() {
        "empty_result"
    } else {
        "ok"
    };
    let dominant_state = dominant_state(&state_summary_ns);
    let mostly_running = window_duration > 0 && running_ns * 10 >= window_duration * 9;
    let wait_chain_not_closed =
        !candidate_wait_segments.is_empty() && new_candidate_edges.is_empty();
    let all_candidate_edges_repeated = candidate_edge_count > 0
        && repeated_candidate_edges.len() == candidate_edge_count
        && !max_depth_reached;
    let pending_edges = next_candidate_edges.len();
    let has_pending_edges = !next_candidate_edges.is_empty();
    let edge_boundary_reason = if segments.is_empty() {
        Some("no_state_segments")
    } else if max_depth_reached {
        Some("max_depth_reached")
    } else if all_candidate_edges_repeated {
        Some("all_candidate_edges_repeated")
    } else if wait_chain_not_closed {
        Some("wait_chain_not_closed")
    } else {
        None
    };

    Ok(json!({
        "schema": schema,
        "status": status,
        "facts": {
            "itid": target_itid,
            "window": {
                "start_ts": start_ts,
                "end_ts": end_ts,
                "duration_ns": window_duration
            },
            "state_summary_ns": state_summary_ns,
            "dominant_state": dominant_state,
            "segments": segments,
            "candidate_wait_segments": candidate_wait_segments,
            "new_candidate_edges": new_candidate_edges,
            "next_candidate_edges": next_candidate_edges,
            "selected_edge_update": selected_edge_update(&params),
            "visited_edges_after": visited_edges_after,
            "coverage": coverage(&params, start_ts, end_ts),
            "edge_boundary_hints": {
                "reason": edge_boundary_reason,
                "depth": depth,
                "max_depth": max_depth,
                "no_state_segments": segments.is_empty(),
                "max_depth_reached": max_depth_reached,
                "all_candidate_edges_repeated": all_candidate_edges_repeated,
                "repeated_candidate_edges": repeated_candidate_edges,
                "mostly_running": mostly_running,
                "wait_chain_not_closed": wait_chain_not_closed
            },
            "frontier_hints": {
                "pending_edges": pending_edges,
                "has_pending_edges": has_pending_edges
            }
        },
        "limitations": []
    }))
}

pub fn profile_sched_slices(schema: &str, params: Value, rows: Vec<Value>) -> Result<Value> {
    let start_ts = i64_field(&params, "start_ts").ok_or_else(|| anyhow!("missing start_ts"))?;
    let end_ts = i64_field(&params, "end_ts").ok_or_else(|| anyhow!("missing end_ts"))?;
    let target_itid = i64_field(&params, "itid");

    let mut sched_running_ns = 0;
    let mut cpu_summary_ns = BTreeMap::<String, i64>::new();
    let mut slices = Vec::new();

    for row in rows {
        if let Some(itid) = target_itid {
            if i64_field(&row, "itid") != Some(itid) {
                continue;
            }
        }

        let Some(ts) = i64_field(&row, "ts") else {
            continue;
        };
        let dur = i64_field(&row, "dur").unwrap_or_default();
        let Some((clip_start_ts, clip_end_ts, duration_ns)) =
            clipped_duration(ts, dur, start_ts, end_ts)
        else {
            continue;
        };

        sched_running_ns += duration_ns;
        let cpu_key = string_field(&row, "cpu").unwrap_or_else(|| "unknown".to_string());
        *cpu_summary_ns.entry(cpu_key).or_default() += duration_ns;

        slices.push(json!({
            "cpu": row.get("cpu").cloned(),
            "itid": i64_field(&row, "itid"),
            "ts": ts,
            "dur": dur,
            "clip_start_ts": clip_start_ts,
            "clip_end_ts": clip_end_ts,
            "duration_ns": duration_ns,
            "clipped_dur_ns": duration_ns,
            "priority": row.get("priority").cloned(),
            "end_state": string_field(&row, "end_state")
        }));
    }

    let status = if slices.is_empty() {
        "empty_result"
    } else {
        "ok"
    };

    Ok(json!({
        "schema": schema,
        "status": status,
        "facts": {
            "itid": target_itid,
            "window": {
                "start_ts": start_ts,
                "end_ts": end_ts,
                "duration_ns": end_ts - start_ts
            },
            "sched_running_ns": sched_running_ns,
            "cpu_summary_ns": cpu_summary_ns,
            "slices": slices
        },
        "limitations": []
    }))
}

pub fn profile_callstack_context(schema: &str, params: Value, rows: Vec<Value>) -> Result<Value> {
    let start_ts = i64_field(&params, "start_ts").ok_or_else(|| anyhow!("missing start_ts"))?;
    let end_ts = i64_field(&params, "end_ts").ok_or_else(|| anyhow!("missing end_ts"))?;
    let target_itid = i64_field(&params, "itid");

    let mut span_infos = Vec::new();
    let mut top_names_by_span_overlap_ns = BTreeMap::<String, i64>::new();
    let mut total_span_overlap_ns = 0;

    for row in rows {
        if let Some(itid) = target_itid {
            if i64_field(&row, "itid") != Some(itid) {
                continue;
            }
        }

        let Some(ts) = i64_field(&row, "ts") else {
            continue;
        };
        let dur = i64_field(&row, "dur").unwrap_or_default();
        let Some((clip_start_ts, clip_end_ts, overlap_ns)) =
            clipped_duration(ts, dur, start_ts, end_ts)
        else {
            continue;
        };

        let name = string_field(&row, "name").unwrap_or_else(|| "<unknown>".to_string());
        *top_names_by_span_overlap_ns
            .entry(name.clone())
            .or_default() += overlap_ns;
        total_span_overlap_ns += overlap_ns;
        span_infos.push(CallstackSpan {
            id: i64_field(&row, "id"),
            parent_id: i64_field(&row, "parent_id"),
            name: name.clone(),
            clip_start_ts,
            clip_end_ts,
            overlap_ns,
            value: json!({
            "id": row.get("id").cloned(),
            "itid": i64_field(&row, "itid"),
            "tid": i64_field(&row, "tid"),
            "thread_name": string_field(&row, "thread_name"),
            "process_name": string_field(&row, "process_name"),
            "ts": ts,
            "dur": dur,
            "clip_start_ts": clip_start_ts,
            "clip_end_ts": clip_end_ts,
            "overlap_ns": overlap_ns,
            "overlap_dur_ns": overlap_ns,
            "name": name,
            "cat": string_field(&row, "cat"),
            "depth": i64_field(&row, "depth"),
            "parent_id": row.get("parent_id").cloned()
            }),
        });
    }

    let mut spans = Vec::new();
    let mut top_names_by_self_overlap_ns = BTreeMap::<String, i64>::new();
    let mut total_self_overlap_ns = 0;
    for (index, span) in span_infos.iter().enumerate() {
        let child_intervals = span
            .id
            .map(|id| {
                span_infos
                    .iter()
                    .enumerate()
                    .filter(|(child_index, child)| {
                        *child_index != index && child.parent_id == Some(id)
                    })
                    .filter_map(|(_, child)| {
                        let start = child.clip_start_ts.max(span.clip_start_ts);
                        let end = child.clip_end_ts.min(span.clip_end_ts);
                        (end > start).then_some((start, end))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let child_overlap_ns = merged_interval_duration(child_intervals);
        let self_overlap_ns = (span.overlap_ns - child_overlap_ns).max(0);
        total_self_overlap_ns += self_overlap_ns;
        *top_names_by_self_overlap_ns
            .entry(span.name.clone())
            .or_default() += self_overlap_ns;

        let mut value = span.value.clone();
        if let Some(object) = value.as_object_mut() {
            object.insert("self_overlap_ns".to_string(), json!(self_overlap_ns));
        }
        spans.push(value);
    }

    let status = if span_infos.is_empty() {
        "empty_result"
    } else {
        "ok"
    };
    let limitations = if span_infos.is_empty() {
        Vec::<Value>::new()
    } else {
        vec![json!(
            "span overlaps are nested and not additive; use self_overlap_ns for additive attribution"
        )]
    };

    Ok(json!({
        "schema": schema,
        "status": status,
        "facts": {
            "itid": target_itid,
            "window": {
                "start_ts": start_ts,
                "end_ts": end_ts,
                "duration_ns": end_ts - start_ts
            },
            "total_span_overlap_ns": total_span_overlap_ns,
            "span_overlap_is_additive": false,
            "top_names_by_span_overlap_ns": top_names_by_span_overlap_ns,
            "total_self_overlap_ns": total_self_overlap_ns,
            "top_names_by_self_overlap_ns": top_names_by_self_overlap_ns,
            "spans": spans
        },
        "limitations": limitations
    }))
}

fn classify_thread_state(state: &str) -> &'static str {
    match state {
        "Running" => "running",
        "R" | "R+" => "runnable",
        "S" => "sleeping",
        "D-IO" => "io_wait",
        "D" => "uninterruptible",
        _ => "unknown",
    }
}

fn dominant_state(summary: &BTreeMap<String, i64>) -> Option<String> {
    summary
        .iter()
        .max_by(|(left_state, left_ns), (right_state, right_ns)| {
            left_ns
                .cmp(right_ns)
                .then_with(|| right_state.cmp(left_state))
        })
        .map(|(state, _)| state.clone())
}

fn string_array(params: &Value, field: &str) -> Vec<String> {
    params
        .get(field)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

fn object_array(params: &Value, field: &str) -> Vec<Value> {
    params
        .get(field)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|value| value.is_object())
        .cloned()
        .collect()
}

fn selected_edge_id(params: &Value) -> Option<String> {
    params
        .get("selected_edge")?
        .get("edge_id")?
        .as_str()
        .map(ToOwned::to_owned)
}

fn selected_edge_update(params: &Value) -> Value {
    let Some(selected_edge) = params
        .get("selected_edge")
        .filter(|value| value.is_object())
    else {
        return Value::Null;
    };
    let mut updated = selected_edge.clone();
    if let Some(object) = updated.as_object_mut() {
        object.insert("status".to_string(), json!("visited"));
    }
    updated
}

fn coverage(params: &Value, start_ts: i64, end_ts: i64) -> Value {
    let root_start_ts = params
        .get("root_window")
        .and_then(|root_window| i64_field(root_window, "start_ts"))
        .unwrap_or(start_ts);
    let root_end_ts = params
        .get("root_window")
        .and_then(|root_window| i64_field(root_window, "end_ts"))
        .unwrap_or(end_ts);
    let root_duration_ns = (root_end_ts - root_start_ts).max(0);
    let explained_ns = merged_interval_duration(
        params
            .get("explained_intervals")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|interval| {
                Some((
                    i64_field(interval, "start_ts")?.max(root_start_ts),
                    i64_field(interval, "end_ts")?.min(root_end_ts),
                ))
            })
            .filter(|(start, end)| end > start)
            .collect(),
    );
    let ratio = if root_duration_ns == 0 {
        0.0
    } else {
        explained_ns as f64 / root_duration_ns as f64
    };

    json!({
        "root_start_ts": root_start_ts,
        "root_end_ts": root_end_ts,
        "root_duration_ns": root_duration_ns,
        "explained_ns": explained_ns,
        "ratio": ratio
    })
}

fn merged_interval_duration(mut intervals: Vec<(i64, i64)>) -> i64 {
    intervals.sort_unstable();
    let mut total = 0;
    let mut current: Option<(i64, i64)> = None;

    for (start, end) in intervals {
        match current {
            None => current = Some((start, end)),
            Some((current_start, current_end)) if start <= current_end => {
                current = Some((current_start, current_end.max(end)));
            }
            Some((current_start, current_end)) => {
                total += current_end - current_start;
                current = Some((start, end));
            }
        }
    }

    if let Some((current_start, current_end)) = current {
        total += current_end - current_start;
    }

    total
}
