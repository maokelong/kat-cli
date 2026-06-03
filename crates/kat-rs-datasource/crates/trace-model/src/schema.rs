use arrow_schema::{DataType, Field, Schema, SchemaRef};
use std::sync::Arc;

pub fn trace_metadata_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("key", DataType::Utf8, false),
        Field::new("value", DataType::Utf8, true),
    ]))
}

pub fn trace_bounds_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("trace_id", DataType::Utf8, false),
        Field::new("start_ts", DataType::Int64, true),
        Field::new("end_ts", DataType::Int64, true),
        Field::new("clock_domain", DataType::Utf8, false),
    ]))
}

pub fn process_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("upid", DataType::UInt32, false),
        Field::new("pid", DataType::UInt32, false),
        Field::new("name", DataType::Utf8, true),
        Field::new("start_ts", DataType::Int64, true),
        Field::new("end_ts", DataType::Int64, true),
    ]))
}

pub fn thread_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("utid", DataType::UInt32, false),
        Field::new("tid", DataType::UInt32, false),
        Field::new("upid", DataType::UInt32, false),
        Field::new("name", DataType::Utf8, true),
        Field::new("is_main", DataType::Boolean, false),
    ]))
}

pub fn sched_slice_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("cpu", DataType::UInt32, false),
        Field::new("utid", DataType::UInt32, false),
        Field::new("ts", DataType::Int64, false),
        Field::new("dur", DataType::Int64, true),
        Field::new("priority", DataType::Int32, true),
        Field::new("end_state", DataType::Utf8, true),
    ]))
}

pub fn thread_state_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("utid", DataType::UInt32, false),
        Field::new("ts", DataType::Int64, false),
        Field::new("dur", DataType::Int64, true),
        Field::new("state", DataType::Utf8, false),
        Field::new("io_wait", DataType::Boolean, true),
        Field::new("blocked_function", DataType::Utf8, true),
        Field::new("waker_utid", DataType::UInt32, true),
    ]))
}

pub fn raw_event_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("ts", DataType::Int64, true),
        Field::new("cpu", DataType::UInt32, true),
        Field::new("tid", DataType::UInt32, true),
        Field::new("event_name", DataType::Utf8, false),
        Field::new("payload_json", DataType::Utf8, true),
    ]))
}

pub fn raw_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("ts", DataType::Int64, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("cpu", DataType::UInt32, false),
        Field::new("itid", DataType::UInt32, true),
    ]))
}

pub fn instant_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("ts", DataType::Int64, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("ref", DataType::UInt32, true),
        Field::new("wakeup_from", DataType::UInt32, true),
        Field::new("ref_type", DataType::Utf8, true),
        Field::new("value", DataType::Float64, true),
    ]))
}

pub fn irq_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("ts", DataType::Int64, false),
        Field::new("dur", DataType::Int64, true),
        Field::new("callid", DataType::Int32, true),
        Field::new("cat", DataType::Utf8, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("depth", DataType::UInt32, true),
        Field::new("cookie", DataType::UInt64, true),
        Field::new("parent_id", DataType::UInt64, true),
        Field::new("argsetid", DataType::UInt64, true),
        Field::new("flag", DataType::Utf8, true),
    ]))
}

pub fn measure_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("type", DataType::Utf8, false),
        Field::new("ts", DataType::Int64, false),
        Field::new("dur", DataType::Int64, true),
        Field::new("value", DataType::Int64, false),
        Field::new("filter_id", DataType::UInt64, false),
    ]))
}

pub fn measure_filter_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("source_arg_set_id", DataType::UInt64, true),
        Field::new("type", DataType::Utf8, false),
    ]))
}

pub fn cpu_measure_filter_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("cpu", DataType::UInt32, false),
    ]))
}

pub fn symbols_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("funcname", DataType::Utf8, false),
        Field::new("addr", DataType::UInt64, false),
    ]))
}

pub fn dma_fence_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("ts", DataType::Int64, false),
        Field::new("dur", DataType::Int64, true),
        Field::new("cat", DataType::Utf8, false),
        Field::new("driver", DataType::Utf8, false),
        Field::new("timeline", DataType::Utf8, false),
        Field::new("context", DataType::UInt32, false),
        Field::new("seqno", DataType::UInt32, false),
    ]))
}

pub fn cpu_usage_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("ts", DataType::Int64, false),
        Field::new("dur", DataType::Int64, true),
        Field::new("total_load", DataType::Float64, false),
        Field::new("user_load", DataType::Float64, false),
        Field::new("system_load", DataType::Float64, false),
        Field::new("process_num", DataType::Int64, false),
    ]))
}

pub fn diskio_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("ts", DataType::Int64, false),
        Field::new("dur", DataType::Int64, true),
        Field::new("rd", DataType::Int64, false),
        Field::new("wr", DataType::Int64, false),
        Field::new("rd_speed", DataType::Float64, false),
        Field::new("wr_speed", DataType::Float64, false),
        Field::new("rd_count", DataType::Int64, false),
        Field::new("wr_count", DataType::Int64, false),
        Field::new("rd_count_speed", DataType::Float64, false),
        Field::new("wr_count_speed", DataType::Float64, false),
    ]))
}

pub fn data_dict_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("data", DataType::Utf8, false),
    ]))
}

pub fn args_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("key", DataType::UInt64, false),
        Field::new("datatype", DataType::UInt32, false),
        Field::new("value", DataType::Int64, false),
        Field::new("argset", DataType::UInt64, false),
    ]))
}

pub fn callstack_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("ts", DataType::Int64, false),
        Field::new("dur", DataType::Int64, true),
        Field::new("callid", DataType::UInt32, true),
        Field::new("cat", DataType::Utf8, true),
        Field::new("name", DataType::Utf8, true),
        Field::new("depth", DataType::UInt32, true),
        Field::new("cookie", DataType::Int64, true),
        Field::new("parent_id", DataType::UInt64, true),
        Field::new("argsetid", DataType::UInt64, true),
        Field::new("chainId", DataType::Utf8, true),
        Field::new("spanId", DataType::Utf8, true),
        Field::new("parentSpanId", DataType::Utf8, true),
        Field::new("flag", DataType::Utf8, true),
        Field::new("trace_level", DataType::Utf8, true),
        Field::new("trace_tag", DataType::Utf8, true),
        Field::new("custom_category", DataType::Utf8, true),
        Field::new("custom_args", DataType::Utf8, true),
        Field::new("child_callid", DataType::UInt64, true),
    ]))
}

pub fn process_measure_schema() -> SchemaRef {
    measure_schema()
}

pub fn sys_mem_measure_schema() -> SchemaRef {
    measure_schema()
}

pub fn process_measure_filter_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("ipid", DataType::UInt32, false),
    ]))
}

pub fn sys_event_filter_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("type", DataType::Utf8, false),
        Field::new("name", DataType::Utf8, false),
    ]))
}

pub fn live_process_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("ts", DataType::Int64, false),
        Field::new("dur", DataType::Int64, false),
        Field::new("cpu_time", DataType::UInt64, false),
        Field::new("process_id", DataType::Int32, false),
        Field::new("process_name", DataType::Utf8, false),
        Field::new("parent_process_id", DataType::Int32, false),
        Field::new("uid", DataType::Int32, false),
        Field::new("user_name", DataType::Utf8, false),
        Field::new("cpu_usage", DataType::Float64, false),
        Field::new("pss_info", DataType::Int32, false),
        Field::new("thread_num", DataType::Int32, false),
        Field::new("disk_writes", DataType::Int64, false),
        Field::new("disk_reads", DataType::Int64, false),
    ]))
}

pub fn js_heap_files_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt32, false),
        Field::new("file_name", DataType::Utf8, false),
        Field::new("start_time", DataType::Int64, false),
        Field::new("end_time", DataType::Int64, false),
        Field::new("self_size", DataType::UInt64, false),
    ]))
}

pub fn js_heap_info_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("file_id", DataType::UInt32, false),
        Field::new("key", DataType::Utf8, false),
        Field::new("type", DataType::UInt32, false),
        Field::new("int_value", DataType::Int32, false),
        Field::new("str_value", DataType::Utf8, false),
    ]))
}

pub fn js_heap_nodes_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("file_id", DataType::UInt32, false),
        Field::new("node_index", DataType::UInt32, false),
        Field::new("type", DataType::UInt32, false),
        Field::new("name", DataType::UInt32, false),
        Field::new("id", DataType::UInt32, false),
        Field::new("self_size", DataType::UInt32, false),
        Field::new("edge_count", DataType::UInt32, false),
        Field::new("trace_node_id", DataType::UInt32, false),
        Field::new("detachedness", DataType::UInt32, false),
    ]))
}

pub fn js_heap_edges_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("file_id", DataType::UInt32, false),
        Field::new("edge_index", DataType::UInt32, false),
        Field::new("type", DataType::UInt32, false),
        Field::new("name_or_index", DataType::UInt32, false),
        Field::new("to_node", DataType::UInt32, false),
        Field::new("from_node_id", DataType::UInt32, false),
        Field::new("to_node_id", DataType::UInt32, false),
    ]))
}

pub fn js_heap_string_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("file_id", DataType::UInt32, false),
        Field::new("file_index", DataType::UInt64, false),
        Field::new("string", DataType::Utf8, false),
    ]))
}

pub fn js_heap_location_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("file_id", DataType::UInt32, false),
        Field::new("object_index", DataType::UInt32, false),
        Field::new("script_id", DataType::UInt32, false),
        Field::new("line", DataType::UInt32, false),
        Field::new("column", DataType::UInt32, false),
    ]))
}

pub fn js_heap_sample_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("file_id", DataType::UInt32, false),
        Field::new("timestamp_us", DataType::UInt64, false),
        Field::new("last_assigned_id", DataType::UInt32, false),
    ]))
}

pub fn js_heap_trace_function_info_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("file_id", DataType::UInt32, false),
        Field::new("function_index", DataType::UInt32, false),
        Field::new("function_id", DataType::UInt32, false),
        Field::new("name", DataType::UInt32, false),
        Field::new("script_name", DataType::UInt32, false),
        Field::new("script_id", DataType::UInt32, false),
        Field::new("line", DataType::UInt32, false),
        Field::new("column", DataType::UInt32, false),
    ]))
}

pub fn js_heap_trace_node_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("file_id", DataType::UInt32, false),
        Field::new("id", DataType::UInt32, false),
        Field::new("function_info_index", DataType::UInt32, false),
        Field::new("count", DataType::UInt32, false),
        Field::new("size", DataType::UInt32, false),
        Field::new("parent_id", DataType::Int32, false),
    ]))
}

pub fn js_config_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("pid", DataType::Int32, false),
        Field::new("type", DataType::Int32, false),
        Field::new("interval", DataType::UInt32, false),
        Field::new("capture_numeric_value", DataType::UInt32, false),
        Field::new("trace_allocation", DataType::UInt32, false),
        Field::new("enable_cpu_profiler", DataType::UInt32, false),
        Field::new("cpu_profiler_interval", DataType::UInt32, false),
    ]))
}

pub fn js_cpu_profiler_node_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("function_id", DataType::UInt32, false),
        Field::new("function_index", DataType::UInt32, false),
        Field::new("script_id", DataType::Utf8, false),
        Field::new("url_index", DataType::UInt64, false),
        Field::new("line_number", DataType::Int32, false),
        Field::new("column_number", DataType::Int32, false),
        Field::new("hit_count", DataType::Int32, false),
        Field::new("children", DataType::Utf8, false),
        Field::new("parent_id", DataType::UInt32, false),
    ]))
}

pub fn js_cpu_profiler_sample_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("function_id", DataType::UInt32, false),
        Field::new("start_time", DataType::Int64, false),
        Field::new("end_time", DataType::Int64, false),
        Field::new("dur", DataType::Int64, false),
    ]))
}
