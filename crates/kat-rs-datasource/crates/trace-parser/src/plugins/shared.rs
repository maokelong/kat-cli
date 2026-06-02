use std::collections::HashMap;
use trace_model::{CallstackRow, TraceTableBuilder};

const DATATYPE_INT: u32 = 0;
const DATATYPE_STRING: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub enum TraceMarker {
    Begin {
        callid: u32,
        name: String,
    },
    End {
        callid: Option<u32>,
    },
    AsyncBegin {
        callid: u32,
        name: String,
        cookie: i64,
    },
    AsyncEnd {
        callid: u32,
        name: String,
        cookie: i64,
    },
    Counter {
        callid: u32,
        name: String,
        value: i64,
    },
}

#[derive(Default)]
pub struct SharedTraceState {
    sync_stack_by_callid: HashMap<u32, Vec<usize>>,
    async_row_by_key: HashMap<(u32, String, i64), usize>,
}

pub fn parse_trace_marker(payload: &str) -> Option<TraceMarker> {
    let payload = payload.trim();
    let mut parts = payload.split('|');
    let marker = parts.next()?;
    match marker {
        "B" => Some(TraceMarker::Begin {
            callid: parts.next()?.trim().parse().ok()?,
            name: parts.collect::<Vec<_>>().join("|"),
        }),
        "E" => Some(TraceMarker::End {
            callid: parts.next().and_then(|part| part.trim().parse().ok()),
        }),
        "S" => {
            let callid = parts.next()?.trim().parse().ok()?;
            let rest = parts.collect::<Vec<_>>().join("|");
            let (name, cookie) = split_name_value(&rest)?;
            Some(TraceMarker::AsyncBegin {
                callid,
                name,
                cookie,
            })
        }
        "F" => {
            let callid = parts.next()?.trim().parse().ok()?;
            let rest = parts.collect::<Vec<_>>().join("|");
            let (name, cookie) = split_name_value(&rest)?;
            Some(TraceMarker::AsyncEnd {
                callid,
                name,
                cookie,
            })
        }
        "C" => Some(TraceMarker::Counter {
            callid: parts.next()?.trim().parse().ok()?,
            name: {
                let name = parts.next()?.to_string();
                if let Some(value) = parts.next() {
                    return Some(TraceMarker::Counter {
                        callid: payload.split('|').nth(1)?.trim().parse().ok()?,
                        name,
                        value: value.trim().parse().ok()?,
                    });
                }
                let (name, _) = name.rsplit_once(' ')?;
                name.trim_end().to_string()
            },
            value: {
                let rest = payload.splitn(3, '|').nth(2)?;
                let (_, value) = rest.rsplit_once(' ')?;
                value.trim().parse().ok()?
            },
        }),
        _ => None,
    }
}

pub fn handle_trace_marker(
    tables: &mut TraceTableBuilder,
    state: &mut SharedTraceState,
    ts: i64,
    default_callid: u32,
    marker: TraceMarker,
) {
    match marker {
        TraceMarker::Begin { callid, name } => {
            let (name, argsetid, custom_args) = split_name_and_args(tables, name);
            let stack = state.sync_stack_by_callid.entry(callid).or_default();
            let parent_id = stack
                .last()
                .and_then(|row_id| tables.callstack_id_at(*row_id));
            let row_id = tables.push_callstack(CallstackRow {
                id: tables.next_callstack_id(),
                ts,
                dur: None,
                callid: Some(callid),
                cat: None,
                name: Some(name),
                depth: Some(stack.len() as u32),
                cookie: None,
                parent_id,
                argsetid,
                chain_id: None,
                span_id: None,
                parent_span_id: None,
                flag: None,
                trace_level: None,
                trace_tag: None,
                custom_category: None,
                custom_args,
                child_callid: None,
            });
            stack.push(row_id);
        }
        TraceMarker::End { callid } => {
            let callid = callid.unwrap_or(default_callid);
            if let Some(stack) = state.sync_stack_by_callid.get_mut(&callid) {
                if let Some(row_id) = stack.pop() {
                    if let Some(row) = tables.callstack_mut(row_id) {
                        row.dur = Some(ts.saturating_sub(row.ts));
                    }
                }
            }
        }
        TraceMarker::AsyncBegin {
            callid,
            name,
            cookie,
        } => {
            let (name, argsetid, custom_args) = split_name_and_args(tables, name);
            let row_id = tables.push_callstack(CallstackRow {
                id: tables.next_callstack_id(),
                ts,
                dur: None,
                callid: Some(callid),
                cat: None,
                name: Some(name.clone()),
                depth: None,
                cookie: Some(cookie),
                parent_id: None,
                argsetid,
                chain_id: None,
                span_id: None,
                parent_span_id: None,
                flag: Some("S".to_string()),
                trace_level: None,
                trace_tag: None,
                custom_category: None,
                custom_args,
                child_callid: Some(default_callid as u64),
            });
            state
                .async_row_by_key
                .insert((callid, name, cookie), row_id);
        }
        TraceMarker::AsyncEnd {
            callid,
            name,
            cookie,
        } => {
            let key = (callid, name, cookie);
            if let Some(row_id) = state.async_row_by_key.remove(&key) {
                if let Some(row) = tables.callstack_mut(row_id) {
                    row.dur = Some(ts.saturating_sub(row.ts));
                    row.flag = Some("F".to_string());
                }
            }
        }
        TraceMarker::Counter {
            callid,
            name,
            value,
        } => {
            let argsetid = counter_args(tables, &name, value);
            tables.push_callstack(CallstackRow {
                id: tables.next_callstack_id(),
                ts,
                dur: Some(0),
                callid: Some(callid),
                cat: Some("counter".to_string()),
                name: Some(name),
                depth: None,
                cookie: None,
                parent_id: None,
                argsetid: Some(argsetid),
                chain_id: None,
                span_id: None,
                parent_span_id: None,
                flag: Some("C".to_string()),
                trace_level: None,
                trace_tag: None,
                custom_category: None,
                custom_args: Some(value.to_string()),
                child_callid: None,
            });
        }
    }
}

fn split_name_and_args(
    tables: &mut TraceTableBuilder,
    raw_name: String,
) -> (String, Option<u64>, Option<String>) {
    let Some((name, args)) = raw_name.split_once("##") else {
        return (raw_name, None, None);
    };
    let args = args.trim();
    if args.is_empty() {
        return (name.to_string(), None, None);
    }
    let argset = tables.next_argset_id();
    for pair in args.split([',', ';']) {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key_id = tables.intern_string(key.trim());
        let value = value.trim();
        if let Ok(number) = value.parse::<i64>() {
            tables.push_arg(key_id, DATATYPE_INT, number, argset);
        } else {
            let value_id = tables.intern_string(value);
            tables.push_arg(key_id, DATATYPE_STRING, value_id as i64, argset);
        }
    }
    (name.to_string(), Some(argset), Some(args.to_string()))
}

fn counter_args(tables: &mut TraceTableBuilder, name: &str, value: i64) -> u64 {
    let argset = tables.next_argset_id();
    let key_id = tables.intern_string(name);
    tables.push_arg(key_id, DATATYPE_INT, value, argset);
    argset
}

fn split_name_value(rest: &str) -> Option<(String, i64)> {
    let (name, value) = rest.rsplit_once('|').or_else(|| rest.rsplit_once(' '))?;
    Some((name.trim_end().to_string(), value.trim().parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_trace_marker_begin() {
        assert_eq!(
            parse_trace_marker("B|42|render##phase=prepare,count=2"),
            Some(TraceMarker::Begin {
                callid: 42,
                name: "render##phase=prepare,count=2".to_string()
            })
        );
    }
}
