use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const KNOWN_TRACE_TABLES: &[&str] = &[
    "trace_metadata",
    "trace_bounds",
    "process",
    "thread",
    "sched_slice",
    "thread_state",
    "raw_event",
    "raw",
    "instant",
    "irq",
    "measure",
    "measure_filter",
    "cpu_measure_filter",
    "symbols",
    "dma_fence",
    "cpu_usage",
    "diskio",
    "data_dict",
    "args",
    "callstack",
    "process_measure",
    "process_measure_filter",
    "sys_mem_measure",
    "sys_event_filter",
    "live_process",
    "js_heap_files",
    "js_heap_info",
    "js_heap_nodes",
    "js_heap_edges",
    "js_heap_string",
    "js_heap_location",
    "js_heap_sample",
    "js_heap_trace_function_info",
    "js_heap_trace_node",
    "js_config",
    "js_cpu_profiler_node",
    "js_cpu_profiler_sample",
    "log",
    "hisysevent_all_event",
    "hisysevent_measure",
    "perf_report",
    "perf_files",
    "perf_thread",
    "perf_sample",
    "perf_callchain",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryParam {
    String(String),
    I64(i64),
    F64(f64),
    Bool(bool),
    TimestampNs(i64),
    DurationNs(i64),
    StringList(Vec<String>),
    I64List(Vec<i64>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryLimits {
    pub timeout_ms: u64,
    pub max_rows_inline: usize,
    pub max_result_bytes_inline: usize,
    pub memory_budget_bytes: Option<u64>,
}

impl Default for QueryLimits {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            max_rows_inline: 10_000,
            max_result_bytes_inline: 1_048_576,
            memory_budget_bytes: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryOutputMode {
    InlineJson,
    Artifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasourceQueryRequest {
    pub sql: String,
    pub params: BTreeMap<String, QueryParam>,
    pub limits: QueryLimits,
    pub output: QueryOutputMode,
    pub required_tables: Vec<String>,
    pub query_tag: Option<String>,
}

impl DatasourceQueryRequest {
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            params: BTreeMap::new(),
            limits: QueryLimits::default(),
            output: QueryOutputMode::InlineJson,
            required_tables: Vec::new(),
            query_tag: None,
        }
    }
}

pub fn infer_required_tables(sql: &str) -> Vec<String> {
    let tokens = sql
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();

    KNOWN_TRACE_TABLES
        .iter()
        .filter(|table| tokens.contains(**table))
        .map(|table| (*table).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_required_tables_from_simple_sql() {
        assert_eq!(
            infer_required_tables("SELECT COUNT(*) AS slices FROM sched_slice WHERE cpu = 0"),
            vec!["sched_slice".to_string()]
        );
    }

    #[test]
    fn infers_raw_event_without_matching_raw_substring() {
        assert_eq!(
            infer_required_tables("SELECT * FROM raw_event LIMIT 1"),
            vec!["raw_event".to_string()]
        );
    }
}
