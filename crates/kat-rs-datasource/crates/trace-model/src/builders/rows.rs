#[derive(Debug, Clone)]
pub struct ProcessRow {
    pub upid: u32,
    pub pid: u32,
    pub name: Option<String>,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ThreadRow {
    pub utid: u32,
    pub tid: u32,
    pub upid: u32,
    pub name: Option<String>,
    pub is_main: bool,
}

#[derive(Debug, Clone)]
pub struct SchedSliceRow {
    pub cpu: u32,
    pub utid: u32,
    pub ts: i64,
    pub dur: Option<i64>,
    pub priority: Option<i32>,
    pub end_state: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ThreadStateRow {
    pub utid: u32,
    pub ts: i64,
    pub dur: Option<i64>,
    pub state: String,
    pub io_wait: Option<bool>,
    pub blocked_function: Option<String>,
    pub waker_utid: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct RawEventRow {
    pub ts: Option<i64>,
    pub cpu: Option<u32>,
    pub tid: Option<u32>,
    pub event_name: String,
    pub payload_json: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RawRow {
    pub id: u64,
    pub ts: i64,
    pub name: String,
    pub cpu: u32,
    pub itid: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct InstantRow {
    pub ts: i64,
    pub name: String,
    pub ref_id: Option<u32>,
    pub wakeup_from: Option<u32>,
    pub ref_type: Option<String>,
    pub value: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct IrqRow {
    pub id: u64,
    pub ts: i64,
    pub dur: Option<i64>,
    pub callid: Option<i32>,
    pub cat: String,
    pub name: String,
    pub depth: Option<u32>,
    pub cookie: Option<u64>,
    pub parent_id: Option<u64>,
    pub argsetid: Option<u64>,
    pub flag: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MeasureRow {
    pub measure_type: String,
    pub ts: i64,
    pub dur: Option<i64>,
    pub value: i64,
    pub filter_id: u64,
}

#[derive(Debug, Clone)]
pub struct MeasureFilterRow {
    pub id: u64,
    pub name: String,
    pub source_arg_set_id: Option<u64>,
    pub filter_type: String,
}

#[derive(Debug, Clone)]
pub struct CpuMeasureFilterRow {
    pub id: u64,
    pub name: String,
    pub cpu: u32,
}

#[derive(Debug, Clone)]
pub struct SymbolsRow {
    pub id: u64,
    pub funcname: String,
    pub addr: u64,
}

#[derive(Debug, Clone)]
pub struct DmaFenceRow {
    pub id: u64,
    pub ts: i64,
    pub dur: Option<i64>,
    pub cat: String,
    pub driver: String,
    pub timeline: String,
    pub context: u32,
    pub seqno: u32,
}

#[derive(Debug, Clone)]
pub struct CpuUsageRow {
    pub ts: i64,
    pub dur: Option<i64>,
    pub total_load: f64,
    pub user_load: f64,
    pub system_load: f64,
    pub process_num: i64,
}

#[derive(Debug, Clone)]
pub struct DiskioRow {
    pub ts: i64,
    pub dur: Option<i64>,
    pub rd: i64,
    pub wr: i64,
    pub rd_speed: f64,
    pub wr_speed: f64,
    pub rd_count: i64,
    pub wr_count: i64,
    pub rd_count_speed: f64,
    pub wr_count_speed: f64,
}

#[derive(Debug, Clone)]
pub struct DataDictRow {
    pub id: u64,
    pub data: String,
}

#[derive(Debug, Clone)]
pub struct ArgsRow {
    pub id: u64,
    pub key: u64,
    pub datatype: u32,
    pub value: i64,
    pub argset: u64,
}

#[derive(Debug, Clone)]
pub struct CallstackRow {
    pub id: u64,
    pub ts: i64,
    pub dur: Option<i64>,
    pub callid: Option<u32>,
    pub cat: Option<String>,
    pub name: Option<String>,
    pub depth: Option<u32>,
    pub cookie: Option<i64>,
    pub parent_id: Option<u64>,
    pub argsetid: Option<u64>,
    pub chain_id: Option<String>,
    pub span_id: Option<String>,
    pub parent_span_id: Option<String>,
    pub flag: Option<String>,
    pub trace_level: Option<String>,
    pub trace_tag: Option<String>,
    pub custom_category: Option<String>,
    pub custom_args: Option<String>,
    pub child_callid: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ProcessMeasureFilterRow {
    pub id: u64,
    pub name: String,
    pub ipid: u32,
}

#[derive(Debug, Clone)]
pub struct SysEventFilterRow {
    pub id: u64,
    pub filter_type: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct LiveProcessRow {
    pub ts: i64,
    pub dur: i64,
    pub cpu_time: u64,
    pub process_id: i32,
    pub process_name: String,
    pub parent_process_id: i32,
    pub uid: i32,
    pub user_name: String,
    pub cpu_usage: f64,
    pub pss_info: i32,
    pub thread_num: i32,
    pub disk_writes: i64,
    pub disk_reads: i64,
}

#[derive(Debug, Clone)]
pub struct JsHeapFilesRow {
    pub id: u32,
    pub file_name: String,
    pub start_time: i64,
    pub end_time: i64,
    pub self_size: u64,
}

#[derive(Debug, Clone)]
pub struct JsHeapInfoRow {
    pub file_id: u32,
    pub key: String,
    pub value_type: u32,
    pub int_value: i32,
    pub str_value: String,
}

#[derive(Debug, Clone)]
pub struct JsHeapNodesRow {
    pub file_id: u32,
    pub node_index: u32,
    pub node_type: u32,
    pub name: u32,
    pub id: u32,
    pub self_size: u32,
    pub edge_count: u32,
    pub trace_node_id: u32,
    pub detachedness: u32,
}

#[derive(Debug, Clone)]
pub struct JsHeapEdgesRow {
    pub file_id: u32,
    pub edge_index: u32,
    pub edge_type: u32,
    pub name_or_index: u32,
    pub to_node: u32,
    pub from_node_id: u32,
    pub to_node_id: u32,
}

#[derive(Debug, Clone)]
pub struct JsHeapStringRow {
    pub file_id: u32,
    pub file_index: u64,
    pub string: String,
}

#[derive(Debug, Clone)]
pub struct JsHeapLocationRow {
    pub file_id: u32,
    pub object_index: u32,
    pub script_id: u32,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone)]
pub struct JsHeapSampleRow {
    pub file_id: u32,
    pub timestamp_us: u64,
    pub last_assigned_id: u32,
}

#[derive(Debug, Clone)]
pub struct JsHeapTraceFunctionInfoRow {
    pub file_id: u32,
    pub function_index: u32,
    pub function_id: u32,
    pub name: u32,
    pub script_name: u32,
    pub script_id: u32,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone)]
pub struct JsHeapTraceNodeRow {
    pub file_id: u32,
    pub id: u32,
    pub function_info_index: u32,
    pub count: u32,
    pub size: u32,
    pub parent_id: i32,
}

#[derive(Debug, Clone)]
pub struct JsConfigRow {
    pub pid: i32,
    pub heap_type: i32,
    pub interval: u32,
    pub capture_numeric_value: u32,
    pub trace_allocation: u32,
    pub enable_cpu_profiler: u32,
    pub cpu_profiler_interval: u32,
}

#[derive(Debug, Clone)]
pub struct JsCpuProfilerNodeRow {
    pub function_id: u32,
    pub function_index: u32,
    pub script_id: String,
    pub url_index: u64,
    pub line_number: i32,
    pub column_number: i32,
    pub hit_count: i32,
    pub children: String,
    pub parent_id: u32,
}

#[derive(Debug, Clone)]
pub struct JsCpuProfilerSampleRow {
    pub id: u64,
    pub function_id: u32,
    pub start_time: i64,
    pub end_time: i64,
    pub dur: i64,
}
