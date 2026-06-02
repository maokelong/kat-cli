use crate::{TraceEngineError, TraceResult};
use prost::Message;
use serde_json::Value;
use std::collections::HashMap;
use trace_model::{
    JsConfigRow, JsCpuProfilerNodeRow, JsCpuProfilerSampleRow, JsHeapEdgesRow, JsHeapFilesRow,
    JsHeapInfoRow, JsHeapLocationRow, JsHeapNodesRow, JsHeapSampleRow, JsHeapStringRow,
    JsHeapTraceFunctionInfoRow, JsHeapTraceNodeRow, TraceTableBuilder,
};

const SNAPSHOT_END: &str = "{\"id\":1,\"result\":{}}";
const TIMELINE_END: &str = "{\"id\":2,\"result\":{}}";
const CPU_PROFILER_START: &str = "{\"id\":3,\"result\":{}}";
const MICRO_TO_NANO: u64 = 1_000;

#[derive(Clone, PartialEq, Message)]
pub struct ArkTSConfig {
    #[prost(int32, tag = "1")]
    pub pid: i32,
    #[prost(enumeration = "HeapType", tag = "2")]
    pub heap_type: i32,
    #[prost(uint32, tag = "3")]
    pub interval: u32,
    #[prost(bool, tag = "4")]
    pub capture_numeric_value: bool,
    #[prost(bool, tag = "5")]
    pub track_allocations: bool,
    #[prost(bool, tag = "6")]
    pub enable_cpu_profiler: bool,
    #[prost(uint32, tag = "7")]
    pub cpu_profiler_interval: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum HeapType {
    Snapshot = 0,
    Timeline = 1,
    Invalid = -1,
}

#[derive(Clone, PartialEq, Message)]
pub struct ArkTSResult {
    #[prost(bytes = "vec", tag = "1")]
    pub result: Vec<u8>,
}

#[derive(Default)]
pub struct ArkTsState {
    heap_type: i32,
    chunk_buffer: String,
    file_id: u32,
    start_time: Option<i64>,
    enable_cpu_profiler: bool,
}

pub fn parse_config(
    data: &[u8],
    tables: &mut TraceTableBuilder,
    state: &mut ArkTsState,
) -> TraceResult<()> {
    let config = ArkTSConfig::decode(data)
        .map_err(|err| TraceEngineError::Parse(format!("failed to decode ArkTSConfig: {err}")))?;
    state.heap_type = config.heap_type;
    state.enable_cpu_profiler = config.enable_cpu_profiler;
    tables.push_js_config(JsConfigRow {
        pid: config.pid,
        heap_type: config.heap_type,
        interval: config.interval,
        capture_numeric_value: u32::from(config.capture_numeric_value),
        trace_allocation: u32::from(config.track_allocations),
        enable_cpu_profiler: u32::from(config.enable_cpu_profiler),
        cpu_profiler_interval: config.cpu_profiler_interval,
    });
    Ok(())
}

pub fn parse_arkts_plugin<F>(
    data: &[u8],
    ts: Option<i64>,
    tables: &mut TraceTableBuilder,
    state: &mut ArkTsState,
    monotonic_to_primary: F,
) -> TraceResult<()>
where
    F: Fn(u64) -> u64,
{
    let result = ArkTSResult::decode(data)
        .map_err(|err| TraceEngineError::Parse(format!("failed to decode ArkTSResult: {err}")))?;
    let payload = String::from_utf8_lossy(&result.result).to_string();

    if payload == SNAPSHOT_END || payload == TIMELINE_END {
        flush_heap_document(ts, tables, state)?;
        return Ok(());
    }

    if payload == CPU_PROFILER_START {
        return Ok(());
    }

    if let Some(chunk) = extract_chunk(&payload)? {
        if state.start_time.is_none() {
            state.start_time = ts;
        }
        state.chunk_buffer.push_str(&chunk);
        return Ok(());
    }

    if let Some(profile) = extract_cpu_profile(&payload)? {
        parse_cpu_profiler(tables, &profile, monotonic_to_primary)?;
        return Ok(());
    }

    if looks_like_heap_document(&payload) {
        state.chunk_buffer = payload;
        if state.start_time.is_none() {
            state.start_time = ts;
        }
        flush_heap_document(ts, tables, state)?;
    }

    Ok(())
}

pub fn parse_heap_json_document(
    tables: &mut TraceTableBuilder,
    file_id: u32,
    document: &str,
) -> TraceResult<u64> {
    let json = serde_json::from_str::<Value>(document)
        .map_err(|err| TraceEngineError::Parse(format!("failed to parse JS heap JSON: {err}")))?;
    let node_field_len = parse_snapshot_info(tables, file_id, &json)?;
    let flat_nodes = parse_nodes(tables, file_id, &json, node_field_len)?;
    parse_edges(tables, file_id, &json, &flat_nodes, node_field_len)?;
    parse_locations(tables, file_id, &json)?;
    parse_samples(tables, file_id, &json)?;
    parse_strings(tables, file_id, &json)?;
    parse_trace_function_infos(tables, file_id, &json)?;
    parse_trace_tree(tables, file_id, &json)?;

    let self_size = tables
        .js_heap_node_self_sizes()
        .filter(|(row_file_id, _)| *row_file_id == file_id)
        .map(|(_, size)| u64::from(size))
        .sum::<u64>();
    Ok(self_size)
}

fn flush_heap_document(
    end_ts: Option<i64>,
    tables: &mut TraceTableBuilder,
    state: &mut ArkTsState,
) -> TraceResult<()> {
    if state.chunk_buffer.trim().is_empty() {
        return Ok(());
    }
    let file_id = state.file_id;
    let document = std::mem::take(&mut state.chunk_buffer);
    let self_size = parse_heap_json_document(tables, file_id, &document)?;
    let file_name = if state.heap_type == HeapType::Timeline as i32 {
        "Timeline".to_string()
    } else {
        format!("Snapshot{file_id}")
    };
    tables.push_js_heap_file(JsHeapFilesRow {
        id: file_id,
        file_name,
        start_time: state.start_time.or(end_ts).unwrap_or_default(),
        end_time: end_ts.or(state.start_time).unwrap_or_default(),
        self_size,
    });
    state.file_id += 1;
    state.start_time = None;
    Ok(())
}

fn extract_chunk(payload: &str) -> TraceResult<Option<String>> {
    if !payload.contains("\"chunk\"") {
        return Ok(None);
    }
    let json = serde_json::from_str::<Value>(payload).map_err(|err| {
        TraceEngineError::Parse(format!("failed to parse ArkTS chunk JSON: {err}"))
    })?;
    let Some(chunk) = json.get("params").and_then(|params| params.get("chunk")) else {
        return Ok(None);
    };
    Ok(Some(match chunk {
        Value::String(value) => value.clone(),
        value => value.to_string(),
    }))
}

fn extract_cpu_profile(payload: &str) -> TraceResult<Option<String>> {
    if !payload.contains("\"profile\"") {
        if looks_like_cpu_profile(payload) {
            return Ok(Some(payload.to_string()));
        }
        return Ok(None);
    }

    let json = serde_json::from_str::<Value>(payload).map_err(|err| {
        TraceEngineError::Parse(format!("failed to parse ArkTS CPU profile envelope: {err}"))
    })?;
    let Some(profile) = json
        .get("result")
        .and_then(|result| result.get("profile"))
        .or_else(|| json.get("profile"))
    else {
        return Ok(None);
    };

    Ok(Some(match profile {
        Value::String(value) => value.clone(),
        value => value.to_string(),
    }))
}

fn looks_like_heap_document(payload: &str) -> bool {
    payload.contains("\"snapshot\"") && payload.contains("\"nodes\"")
}

fn looks_like_cpu_profile(payload: &str) -> bool {
    payload.contains("\"nodes\"")
        && payload.contains("\"samples\"")
        && payload.contains("\"timeDeltas\"")
}

fn parse_cpu_profiler<F>(
    tables: &mut TraceTableBuilder,
    profile: &str,
    monotonic_to_primary: F,
) -> TraceResult<()>
where
    F: Fn(u64) -> u64,
{
    if profile.trim().is_empty() {
        return Ok(());
    }
    let json = serde_json::from_str::<Value>(profile.trim()).map_err(|err| {
        TraceEngineError::Parse(format!("failed to parse JS CPU profile JSON: {err}"))
    })?;
    parse_cpu_profiler_nodes(tables, &json)?;
    parse_cpu_profiler_samples(tables, &json, monotonic_to_primary)?;
    Ok(())
}

fn parse_cpu_profiler_nodes(tables: &mut TraceTableBuilder, json: &Value) -> TraceResult<()> {
    let Some(nodes) = json.get("nodes").and_then(Value::as_array) else {
        return Ok(());
    };

    let mut parent_by_id = HashMap::new();
    for node in nodes {
        let parent_id = node_u32(node, "id").unwrap_or_default();
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            for child in children {
                if let Some(child_id) = child.as_u64() {
                    parent_by_id.insert(child_id as u32, parent_id);
                }
            }
        }
    }

    for node in nodes {
        let function_id = node_u32(node, "id").unwrap_or_default();
        let call_frame = node.get("callFrame").unwrap_or(&Value::Null);
        let function_name = call_frame
            .get("functionName")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let url = call_frame
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let script_id = call_frame
            .get("scriptId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let children = node
            .get("children")
            .and_then(Value::as_array)
            .map(|children| {
                children
                    .iter()
                    .filter_map(Value::as_u64)
                    .map(|child| child.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let function_index = tables.intern_string(function_name) as u32;
        let url_index = tables.intern_string(url);

        tables.push_js_cpu_profiler_node(JsCpuProfilerNodeRow {
            function_id,
            function_index,
            script_id,
            url_index,
            line_number: value_i32(call_frame.get("lineNumber")).unwrap_or_default(),
            column_number: value_i32(call_frame.get("columnNumber")).unwrap_or_default(),
            hit_count: value_i32(node.get("hitCount")).unwrap_or_default(),
            children,
            parent_id: parent_by_id.get(&function_id).copied().unwrap_or_default(),
        });
    }
    Ok(())
}

fn parse_cpu_profiler_samples<F>(
    tables: &mut TraceTableBuilder,
    json: &Value,
    monotonic_to_primary: F,
) -> TraceResult<()>
where
    F: Fn(u64) -> u64,
{
    let Some(samples) = json.get("samples").and_then(Value::as_array) else {
        return Ok(());
    };
    if samples.is_empty() {
        return Ok(());
    }

    let time_deltas = json
        .get("timeDeltas")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut start_time_us = json
        .get("startTime")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let mut sample_end_time_us = start_time_us;
    let mut sample = samples.first().and_then(Value::as_u64).unwrap_or_default() as u32;

    for (index, value) in samples.iter().enumerate() {
        let current = value.as_u64().unwrap_or_default() as u32;
        if index > 0 && sample != current {
            push_cpu_profiler_sample(
                tables,
                sample,
                start_time_us,
                sample_end_time_us,
                &monotonic_to_primary,
            );
            sample = current;
            start_time_us = sample_end_time_us;
        }
        if let Some(delta_us) = time_deltas.get(index + 1).and_then(Value::as_u64) {
            sample_end_time_us = sample_end_time_us.saturating_add(delta_us);
        }
    }

    push_cpu_profiler_sample(
        tables,
        sample,
        start_time_us,
        sample_end_time_us,
        &monotonic_to_primary,
    );
    Ok(())
}

fn push_cpu_profiler_sample<F>(
    tables: &mut TraceTableBuilder,
    function_id: u32,
    start_time_us: u64,
    end_time_us: u64,
    monotonic_to_primary: &F,
) where
    F: Fn(u64) -> u64,
{
    let start_mono_ns = start_time_us.saturating_mul(MICRO_TO_NANO);
    let end_mono_ns = end_time_us.saturating_mul(MICRO_TO_NANO);
    let start_time = monotonic_to_primary(start_mono_ns) as i64;
    let end_time = monotonic_to_primary(end_mono_ns) as i64;
    let dur = end_mono_ns.saturating_sub(start_mono_ns) as i64;
    tables.push_js_cpu_profiler_sample(JsCpuProfilerSampleRow {
        id: tables.next_js_cpu_profiler_sample_id(),
        function_id,
        start_time,
        end_time,
        dur,
    });
}

fn node_u32(node: &Value, key: &str) -> Option<u32> {
    node.get(key)
        .and_then(Value::as_u64)
        .map(|value| value as u32)
}

fn value_i32(value: Option<&Value>) -> Option<i32> {
    value
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn parse_snapshot_info(
    tables: &mut TraceTableBuilder,
    file_id: u32,
    json: &Value,
) -> TraceResult<usize> {
    let snapshot = json
        .get("snapshot")
        .ok_or_else(|| TraceEngineError::Parse("missing snapshot object".to_string()))?;
    let meta = snapshot
        .get("meta")
        .ok_or_else(|| TraceEngineError::Parse("missing snapshot.meta object".to_string()))?;

    parse_types(tables, file_id, "node_types", meta.get("node_types"));
    parse_types(tables, file_id, "edge_types", meta.get("edge_types"));

    for key in ["node_count", "edge_count", "trace_function_count"] {
        let value = snapshot
            .get(key)
            .and_then(Value::as_i64)
            .unwrap_or_default();
        tables.push_js_heap_info(JsHeapInfoRow {
            file_id,
            key: key.to_string(),
            value_type: 0,
            int_value: i32::try_from(value).unwrap_or(i32::MAX),
            str_value: String::new(),
        });
    }

    Ok(meta
        .get("node_fields")
        .and_then(Value::as_array)
        .map(|fields| fields.len())
        .unwrap_or(7))
}

fn parse_types(tables: &mut TraceTableBuilder, file_id: u32, key: &str, value: Option<&Value>) {
    let Some(values) = value.and_then(Value::as_array) else {
        return;
    };
    for (index, item) in values.iter().enumerate() {
        if index == 0 {
            if let Some(items) = item.as_array() {
                for type_value in items {
                    tables.push_js_heap_info(JsHeapInfoRow {
                        file_id,
                        key: key.to_string(),
                        value_type: 0,
                        int_value: -1,
                        str_value: json_string(type_value),
                    });
                }
            }
        } else {
            tables.push_js_heap_info(JsHeapInfoRow {
                file_id,
                key: key.to_string(),
                value_type: 1,
                int_value: -1,
                str_value: json_string(item.as_array().and_then(|arr| arr.first()).unwrap_or(item)),
            });
        }
    }
}

fn parse_nodes(
    tables: &mut TraceTableBuilder,
    file_id: u32,
    json: &Value,
    node_field_len: usize,
) -> TraceResult<Vec<u32>> {
    let flat = numeric_array(json, "nodes")?;
    for (index, chunk) in flat.chunks(node_field_len).enumerate() {
        if chunk.len() < 7 {
            continue;
        }
        tables.push_js_heap_node(JsHeapNodesRow {
            file_id,
            node_index: index as u32,
            node_type: chunk[0],
            name: chunk[1],
            id: chunk[2],
            self_size: chunk[3],
            edge_count: chunk[4],
            trace_node_id: chunk[5],
            detachedness: chunk[6],
        });
    }
    Ok(flat)
}

fn parse_edges(
    tables: &mut TraceTableBuilder,
    file_id: u32,
    json: &Value,
    flat_nodes: &[u32],
    node_field_len: usize,
) -> TraceResult<()> {
    let flat_edges = numeric_array(json, "edges")?;
    let mut from_node_ids = Vec::new();
    for node in flat_nodes.chunks(node_field_len) {
        if node.len() < 5 {
            continue;
        }
        for _ in 0..node[4] {
            from_node_ids.push(node[2]);
        }
    }
    for (index, edge) in flat_edges.chunks(3).enumerate() {
        if edge.len() < 3 {
            continue;
        }
        let to_node = edge[2] as usize;
        let to_node_id = flat_nodes.get(to_node + 2).copied().unwrap_or_default();
        tables.push_js_heap_edge(JsHeapEdgesRow {
            file_id,
            edge_index: index as u32,
            edge_type: edge[0],
            name_or_index: edge[1],
            to_node: edge[2],
            from_node_id: from_node_ids.get(index).copied().unwrap_or_default(),
            to_node_id,
        });
    }
    Ok(())
}

fn parse_locations(tables: &mut TraceTableBuilder, file_id: u32, json: &Value) -> TraceResult<()> {
    let flat = numeric_array(json, "locations")?;
    for location in flat.chunks(4) {
        if location.len() < 4 {
            continue;
        }
        tables.push_js_heap_location(JsHeapLocationRow {
            file_id,
            object_index: location[0],
            script_id: location[1],
            line: location[2],
            column: location[3],
        });
    }
    Ok(())
}

fn parse_samples(tables: &mut TraceTableBuilder, file_id: u32, json: &Value) -> TraceResult<()> {
    let flat = numeric_array(json, "samples")?;
    for sample in flat.chunks(2) {
        if sample.len() < 2 {
            continue;
        }
        tables.push_js_heap_sample(JsHeapSampleRow {
            file_id,
            timestamp_us: u64::from(sample[0]),
            last_assigned_id: sample[1],
        });
    }
    Ok(())
}

fn parse_strings(tables: &mut TraceTableBuilder, file_id: u32, json: &Value) -> TraceResult<()> {
    let Some(strings) = json.get("strings").and_then(Value::as_array) else {
        return Ok(());
    };
    for (index, string) in strings.iter().enumerate() {
        tables.push_js_heap_string(JsHeapStringRow {
            file_id,
            file_index: index as u64,
            string: json_string(string),
        });
    }
    Ok(())
}

fn parse_trace_function_infos(
    tables: &mut TraceTableBuilder,
    file_id: u32,
    json: &Value,
) -> TraceResult<()> {
    let flat = numeric_array(json, "trace_function_infos")?;
    for (index, info) in flat.chunks(6).enumerate() {
        if info.len() < 6 {
            continue;
        }
        tables.push_js_heap_trace_function_info(JsHeapTraceFunctionInfoRow {
            file_id,
            function_index: index as u32,
            function_id: info[0],
            name: info[1],
            script_name: info[2],
            script_id: info[3],
            line: info[4],
            column: info[5],
        });
    }
    Ok(())
}

fn parse_trace_tree(tables: &mut TraceTableBuilder, file_id: u32, json: &Value) -> TraceResult<()> {
    let Some(tree) = json.get("trace_tree").and_then(Value::as_array) else {
        return Ok(());
    };
    parse_trace_tree_array(tables, file_id, tree, -1);
    Ok(())
}

fn parse_trace_tree_array(
    tables: &mut TraceTableBuilder,
    file_id: u32,
    array: &[Value],
    parent_id: i32,
) {
    for item in array.chunks(5) {
        if item.len() < 5 {
            continue;
        }
        let id = item[0].as_u64().unwrap_or_default() as u32;
        tables.push_js_heap_trace_node(JsHeapTraceNodeRow {
            file_id,
            id,
            function_info_index: item[1].as_u64().unwrap_or_default() as u32,
            count: item[2].as_u64().unwrap_or_default() as u32,
            size: item[3].as_u64().unwrap_or_default() as u32,
            parent_id,
        });
        if let Some(children) = item[4].as_array() {
            parse_trace_tree_array(tables, file_id, children, id as i32);
        }
    }
}

fn numeric_array(json: &Value, key: &str) -> TraceResult<Vec<u32>> {
    let Some(values) = json.get(key).and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    Ok(values
        .iter()
        .filter_map(|value| value.as_u64().map(|value| value as u32))
        .collect())
}

fn json_string(value: &Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
}
