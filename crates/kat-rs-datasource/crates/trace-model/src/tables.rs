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

pub fn table_names() -> &'static [&'static str] {
    TRACE_TABLE_NAMES
}

pub fn is_trace_table(table_name: &str) -> bool {
    TRACE_TABLE_NAMES.contains(&table_name)
}
