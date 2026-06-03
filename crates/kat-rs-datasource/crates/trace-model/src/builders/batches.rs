use super::rows::*;
use super::ModelResult;
use crate::schema::*;
use arrow_array::{
    ArrayRef, BooleanArray, Float64Array, Int32Array, Int64Array, RecordBatch, StringArray,
    UInt32Array, UInt64Array,
};
use std::sync::Arc;

pub(super) fn metadata_batch(rows: Vec<(String, Option<String>)>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        trace_metadata_schema(),
        vec![
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.1.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn trace_bounds_batch(
    trace_id: String,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    clock_domain: String,
) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        trace_bounds_schema(),
        vec![
            Arc::new(StringArray::from(vec![trace_id])) as ArrayRef,
            Arc::new(Int64Array::from(vec![start_ts])) as ArrayRef,
            Arc::new(Int64Array::from(vec![end_ts])) as ArrayRef,
            Arc::new(StringArray::from(vec![clock_domain])) as ArrayRef,
        ],
    )
}

pub(super) fn process_batch(rows: Vec<ProcessRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        process_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.upid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.pid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.name.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.start_ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.end_ts).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn thread_batch(rows: Vec<ThreadRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        thread_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.utid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.tid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.upid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.name.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(BooleanArray::from(
                rows.iter().map(|r| r.is_main).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn sched_slice_batch(rows: Vec<SchedSliceRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        sched_slice_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.cpu).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.utid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.dur).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.priority).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.end_state.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn thread_state_batch(rows: Vec<ThreadStateRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        thread_state_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.utid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.dur).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.state.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(BooleanArray::from(
                rows.iter().map(|r| r.io_wait).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.blocked_function.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.waker_utid).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn raw_event_batch(rows: Vec<RawEventRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        raw_event_schema(),
        vec![
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.cpu).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.tid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.event_name.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.payload_json.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn raw_batch(rows: Vec<RawRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        raw_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.cpu).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.itid).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn instant_batch(rows: Vec<InstantRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        instant_schema(),
        vec![
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.ref_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.wakeup_from).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.ref_type.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(Float64Array::from(
                rows.iter().map(|r| r.value).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn irq_batch(rows: Vec<IrqRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        irq_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.dur).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.callid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.cat.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.depth).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.cookie).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.parent_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.argsetid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.flag.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn measure_batch(rows: Vec<MeasureRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        measure_schema(),
        vec![
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.measure_type.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.dur).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.value).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.filter_id).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn measure_filter_batch(rows: Vec<MeasureFilterRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        measure_filter_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.source_arg_set_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.filter_type.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn cpu_measure_filter_batch(rows: Vec<CpuMeasureFilterRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        cpu_measure_filter_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.cpu).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn symbols_batch(rows: Vec<SymbolsRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        symbols_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.funcname.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.addr).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn dma_fence_batch(rows: Vec<DmaFenceRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        dma_fence_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.dur).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.cat.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.driver.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.timeline.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.context).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.seqno).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn cpu_usage_batch(rows: Vec<CpuUsageRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        cpu_usage_schema(),
        vec![
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.dur).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Float64Array::from(
                rows.iter().map(|r| r.total_load).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Float64Array::from(
                rows.iter().map(|r| r.user_load).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Float64Array::from(
                rows.iter().map(|r| r.system_load).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.process_num).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn diskio_batch(rows: Vec<DiskioRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        diskio_schema(),
        vec![
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.dur).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.rd).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.wr).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Float64Array::from(
                rows.iter().map(|r| r.rd_speed).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Float64Array::from(
                rows.iter().map(|r| r.wr_speed).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.rd_count).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.wr_count).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Float64Array::from(
                rows.iter().map(|r| r.rd_count_speed).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Float64Array::from(
                rows.iter().map(|r| r.wr_count_speed).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn data_dict_batch(rows: Vec<DataDictRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        data_dict_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.data.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn args_batch(rows: Vec<ArgsRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        args_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.key).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.datatype).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.value).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.argset).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn callstack_batch(rows: Vec<CallstackRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        callstack_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.dur).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.callid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.cat.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.name.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.depth).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.cookie).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.parent_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.argsetid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.chain_id.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.span_id.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.parent_span_id.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.flag.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.trace_level.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.trace_tag.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.custom_category.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.custom_args.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.child_callid).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn process_measure_batch(rows: Vec<MeasureRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        process_measure_schema(),
        vec![
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.measure_type.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.dur).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.value).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.filter_id).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn sys_mem_measure_batch(rows: Vec<MeasureRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        sys_mem_measure_schema(),
        vec![
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.measure_type.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.dur).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.value).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.filter_id).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn process_measure_filter_batch(
    rows: Vec<ProcessMeasureFilterRow>,
) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        process_measure_filter_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.ipid).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn sys_event_filter_batch(rows: Vec<SysEventFilterRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        sys_event_filter_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.filter_type.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn live_process_batch(rows: Vec<LiveProcessRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        live_process_schema(),
        vec![
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.dur).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.cpu_time).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.process_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.process_name.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.parent_process_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.uid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.user_name.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Float64Array::from(
                rows.iter().map(|r| r.cpu_usage).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.pss_info).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.thread_num).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.disk_writes).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.disk_reads).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn js_heap_files_batch(rows: Vec<JsHeapFilesRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        js_heap_files_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.file_name.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.start_time).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.end_time).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.self_size).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn js_heap_info_batch(rows: Vec<JsHeapInfoRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        js_heap_info_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.file_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.value_type).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.int_value).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.str_value.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn js_heap_nodes_batch(rows: Vec<JsHeapNodesRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        js_heap_nodes_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.file_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.node_index).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.node_type).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.name).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.self_size).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.edge_count).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.trace_node_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.detachedness).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn js_heap_edges_batch(rows: Vec<JsHeapEdgesRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        js_heap_edges_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.file_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.edge_index).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.edge_type).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.name_or_index).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.to_node).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.from_node_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.to_node_id).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn js_heap_string_batch(rows: Vec<JsHeapStringRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        js_heap_string_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.file_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.file_index).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.string.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn js_heap_location_batch(rows: Vec<JsHeapLocationRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        js_heap_location_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.file_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.object_index).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.script_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.line).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.column).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn js_heap_sample_batch(rows: Vec<JsHeapSampleRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        js_heap_sample_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.file_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.timestamp_us).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.last_assigned_id).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn js_heap_trace_function_info_batch(
    rows: Vec<JsHeapTraceFunctionInfoRow>,
) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        js_heap_trace_function_info_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.file_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.function_index).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.function_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.name).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.script_name).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.script_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.line).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.column).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn js_heap_trace_node_batch(rows: Vec<JsHeapTraceNodeRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        js_heap_trace_node_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.file_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter()
                    .map(|r| r.function_info_index)
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.count).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.size).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.parent_id).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn js_config_batch(rows: Vec<JsConfigRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        js_config_schema(),
        vec![
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.pid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.heap_type).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.interval).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter()
                    .map(|r| r.capture_numeric_value)
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.trace_allocation).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter()
                    .map(|r| r.enable_cpu_profiler)
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter()
                    .map(|r| r.cpu_profiler_interval)
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn js_cpu_profiler_node_batch(
    rows: Vec<JsCpuProfilerNodeRow>,
) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        js_cpu_profiler_node_schema(),
        vec![
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.function_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.function_index).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.script_id.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.url_index).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.line_number).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.column_number).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.hit_count).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.children.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.parent_id).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

pub(super) fn js_cpu_profiler_sample_batch(
    rows: Vec<JsCpuProfilerSampleRow>,
) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        js_cpu_profiler_sample_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.function_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.start_time).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.end_time).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.dur).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}
