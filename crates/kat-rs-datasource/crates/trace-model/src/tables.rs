use arrow_array::RecordBatch;
use std::collections::BTreeMap;

pub const TRACE_TABLE_NAMES: &[&str] = &[
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
    "dma_fence",
    "data_dict",
    "args",
    "callstack",
    "process_measure",
    "process_measure_filter",
];

#[derive(Debug, Clone)]
pub struct TraceTables {
    pub trace_metadata: RecordBatch,
    pub trace_bounds: RecordBatch,
    pub process: RecordBatch,
    pub thread: RecordBatch,
    pub sched_slice: RecordBatch,
    pub thread_state: RecordBatch,
    pub raw_event: RecordBatch,
    pub raw: RecordBatch,
    pub instant: RecordBatch,
    pub irq: RecordBatch,
    pub measure: RecordBatch,
    pub measure_filter: RecordBatch,
    pub cpu_measure_filter: RecordBatch,
    pub symbols: RecordBatch,
    pub dma_fence: RecordBatch,
    pub cpu_usage: RecordBatch,
    pub diskio: RecordBatch,
    pub data_dict: RecordBatch,
    pub args: RecordBatch,
    pub callstack: RecordBatch,
    pub process_measure: RecordBatch,
    pub process_measure_filter: RecordBatch,
    pub sys_mem_measure: RecordBatch,
    pub sys_event_filter: RecordBatch,
    pub live_process: RecordBatch,
    pub js_heap_files: RecordBatch,
    pub js_heap_info: RecordBatch,
    pub js_heap_nodes: RecordBatch,
    pub js_heap_edges: RecordBatch,
    pub js_heap_string: RecordBatch,
    pub js_heap_location: RecordBatch,
    pub js_heap_sample: RecordBatch,
    pub js_heap_trace_function_info: RecordBatch,
    pub js_heap_trace_node: RecordBatch,
    pub js_config: RecordBatch,
    pub js_cpu_profiler_node: RecordBatch,
    pub js_cpu_profiler_sample: RecordBatch,
}

impl TraceTables {
    pub fn batches(&self) -> BTreeMap<&'static str, RecordBatch> {
        BTreeMap::from([
            ("trace_metadata", self.trace_metadata.clone()),
            ("trace_bounds", self.trace_bounds.clone()),
            ("process", self.process.clone()),
            ("thread", self.thread.clone()),
            ("sched_slice", self.sched_slice.clone()),
            ("thread_state", self.thread_state.clone()),
            ("raw_event", self.raw_event.clone()),
            ("raw", self.raw.clone()),
            ("instant", self.instant.clone()),
            ("irq", self.irq.clone()),
            ("measure", self.measure.clone()),
            ("measure_filter", self.measure_filter.clone()),
            ("cpu_measure_filter", self.cpu_measure_filter.clone()),
            ("dma_fence", self.dma_fence.clone()),
            ("data_dict", self.data_dict.clone()),
            ("args", self.args.clone()),
            ("callstack", self.callstack.clone()),
            ("process_measure", self.process_measure.clone()),
            (
                "process_measure_filter",
                self.process_measure_filter.clone(),
            ),
        ])
    }
}

#[derive(Debug, Clone)]
pub struct ParsedTrace {
    pub trace_id: String,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub clock_domain: String,
    pub tables: TraceTables,
}

impl ParsedTrace {
    pub fn batches(&self) -> BTreeMap<&'static str, RecordBatch> {
        self.tables.batches()
    }
}
