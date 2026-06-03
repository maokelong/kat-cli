use arrow_schema::{DataType, Field, Schema, SchemaRef};
use std::sync::Arc;

pub fn schema_for_table(table_name: &str) -> Option<SchemaRef> {
    match table_name {
        "trace_metadata" => Some(trace_metadata_schema()),
        "trace_bounds" => Some(trace_bounds_schema()),
        "process" => Some(process_schema()),
        "thread" => Some(thread_schema()),
        "sched_slice" => Some(sched_slice_schema()),
        "thread_state" => Some(thread_state_schema()),
        "raw_event" => Some(raw_event_schema()),
        "raw" => Some(raw_schema()),
        "instant" => Some(instant_schema()),
        "irq" => Some(irq_schema()),
        "measure" => Some(measure_schema()),
        "measure_filter" => Some(measure_filter_schema()),
        "cpu_measure_filter" => Some(cpu_measure_filter_schema()),
        "dma_fence" => Some(dma_fence_schema()),
        "data_dict" => Some(data_dict_schema()),
        "args" => Some(args_schema()),
        "callstack" => Some(callstack_schema()),
        "process_measure" => Some(process_measure_schema()),
        "process_measure_filter" => Some(process_measure_filter_schema()),
        _ => None,
    }
}

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
    Arc::new(Schema::new(vec![
        Field::new("type", DataType::Utf8, false),
        Field::new("ts", DataType::Int64, false),
        Field::new("dur", DataType::Int64, true),
        Field::new("value", DataType::Int64, false),
        Field::new("filter_id", DataType::UInt64, false),
    ]))
}

pub fn process_measure_filter_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("ipid", DataType::UInt32, false),
    ]))
}
