use crate::schema::*;
use arrow_array::{
    ArrayRef, BooleanArray, Float64Array, Int32Array, Int64Array, RecordBatch, StringArray,
    UInt32Array, UInt64Array,
};
use std::collections::HashMap;
use std::sync::Arc;

mod rows;
pub use rows::*;

pub type ModelResult<T> = Result<T, arrow_schema::ArrowError>;

#[derive(Default)]
pub struct TraceTableBuilder {
    metadata: Vec<(String, Option<String>)>,
    processes: Vec<ProcessRow>,
    threads: Vec<ThreadRow>,
    sched_slices: Vec<SchedSliceRow>,
    thread_states: Vec<ThreadStateRow>,
    raw_events: Vec<RawEventRow>,
    raw_rows: Vec<RawRow>,
    instants: Vec<InstantRow>,
    irqs: Vec<IrqRow>,
    measures: Vec<MeasureRow>,
    measure_filters: Vec<MeasureFilterRow>,
    cpu_measure_filters: Vec<CpuMeasureFilterRow>,
    symbols: Vec<SymbolsRow>,
    dma_fences: Vec<DmaFenceRow>,
    cpu_usages: Vec<CpuUsageRow>,
    diskios: Vec<DiskioRow>,
    data_dict: Vec<DataDictRow>,
    data_dict_index: HashMap<String, u64>,
    args: Vec<ArgsRow>,
    callstacks: Vec<CallstackRow>,
    process_measures: Vec<MeasureRow>,
    process_measure_filters: Vec<ProcessMeasureFilterRow>,
    sys_mem_measures: Vec<MeasureRow>,
    sys_event_filters: Vec<SysEventFilterRow>,
    live_processes: Vec<LiveProcessRow>,
    js_heap_files: Vec<JsHeapFilesRow>,
    js_heap_info: Vec<JsHeapInfoRow>,
    js_heap_nodes: Vec<JsHeapNodesRow>,
    js_heap_edges: Vec<JsHeapEdgesRow>,
    js_heap_string: Vec<JsHeapStringRow>,
    js_heap_location: Vec<JsHeapLocationRow>,
    js_heap_sample: Vec<JsHeapSampleRow>,
    js_heap_trace_function_info: Vec<JsHeapTraceFunctionInfoRow>,
    js_heap_trace_node: Vec<JsHeapTraceNodeRow>,
    js_config: Vec<JsConfigRow>,
    js_cpu_profiler_node: Vec<JsCpuProfilerNodeRow>,
    js_cpu_profiler_sample: Vec<JsCpuProfilerSampleRow>,
    logs: Vec<LogRow>,
    hisysevent_all_events: Vec<HiSysEventAllRow>,
    hisysevent_measures: Vec<HiSysEventMeasureRow>,
    perf_reports: Vec<PerfReportRow>,
    perf_files: Vec<PerfFilesRow>,
    perf_threads: Vec<PerfThreadRow>,
    perf_samples: Vec<PerfSampleRow>,
    perf_callchains: Vec<PerfCallchainRow>,
}

impl TraceTableBuilder {
    pub fn reserve_bytrace_rows(&mut self, estimated_lines: usize, include_raw_events: bool) {
        let estimated_sched_rows = (estimated_lines / 4).max(128);
        let estimated_thread_state_rows = (estimated_lines / 2).max(128);
        self.sched_slices.reserve(estimated_sched_rows);
        self.thread_states.reserve(estimated_thread_state_rows);
        self.processes.reserve(1024);
        self.threads.reserve(1024);
        self.data_dict.reserve(1024);
        if include_raw_events {
            self.raw_events.reserve(estimated_lines);
        }
    }

    pub fn push_metadata(&mut self, key: impl Into<String>, value: Option<impl Into<String>>) {
        self.metadata.push((key.into(), value.map(Into::into)));
    }

    pub fn push_process(&mut self, row: ProcessRow) {
        if let Some(name) = &row.name {
            self.intern_string(name);
        }
        self.processes.push(row);
    }

    pub fn push_thread(&mut self, row: ThreadRow) {
        if let Some(name) = &row.name {
            self.intern_string(name);
        }
        self.threads.push(row);
    }

    pub fn push_sched_slice(&mut self, row: SchedSliceRow) -> usize {
        self.sched_slices.push(row);
        self.sched_slices.len() - 1
    }

    pub fn sched_slice_mut(&mut self, row_id: usize) -> Option<&mut SchedSliceRow> {
        self.sched_slices.get_mut(row_id)
    }

    pub fn push_thread_state(&mut self, row: ThreadStateRow) -> usize {
        self.thread_states.push(row);
        self.thread_states.len() - 1
    }

    pub fn thread_state_mut(&mut self, row_id: usize) -> Option<&mut ThreadStateRow> {
        self.thread_states.get_mut(row_id)
    }

    pub fn push_raw_event(&mut self, row: RawEventRow) {
        self.intern_string(&row.event_name);
        self.raw_events.push(row);
    }

    pub fn next_raw_id(&self) -> u64 {
        self.raw_rows.len() as u64
    }

    pub fn push_raw(&mut self, row: RawRow) {
        self.intern_string(&row.name);
        self.raw_rows.push(row);
    }

    pub fn push_instant(&mut self, row: InstantRow) {
        self.intern_string(&row.name);
        if let Some(ref_type) = &row.ref_type {
            self.intern_string(ref_type);
        }
        self.instants.push(row);
    }

    pub fn next_irq_id(&self) -> u64 {
        self.irqs.len() as u64
    }

    pub fn push_irq(&mut self, row: IrqRow) -> usize {
        self.intern_string(&row.cat);
        self.intern_string(&row.name);
        self.irqs.push(row);
        self.irqs.len() - 1
    }

    pub fn irq_mut(&mut self, row_id: usize) -> Option<&mut IrqRow> {
        self.irqs.get_mut(row_id)
    }

    pub fn push_measure(&mut self, row: MeasureRow) -> usize {
        self.measures.push(row);
        self.measures.len() - 1
    }

    pub fn measure_mut(&mut self, row_id: usize) -> Option<&mut MeasureRow> {
        self.measures.get_mut(row_id)
    }

    pub fn push_measure_filter(&mut self, row: MeasureFilterRow) {
        self.intern_string(&row.name);
        self.intern_string(&row.filter_type);
        self.measure_filters.push(row);
    }

    pub fn push_cpu_measure_filter(&mut self, row: CpuMeasureFilterRow) {
        self.intern_string(&row.name);
        self.cpu_measure_filters.push(row);
    }

    pub fn next_symbol_id(&self) -> u64 {
        self.symbols.len() as u64
    }

    pub fn push_symbol(&mut self, row: SymbolsRow) {
        self.intern_string(&row.funcname);
        self.symbols.push(row);
    }

    pub fn next_dma_fence_id(&self) -> u64 {
        self.dma_fences.len() as u64
    }

    pub fn push_dma_fence(&mut self, row: DmaFenceRow) {
        self.intern_string(&row.cat);
        self.intern_string(&row.driver);
        self.intern_string(&row.timeline);
        self.dma_fences.push(row);
    }

    pub fn push_cpu_usage(&mut self, row: CpuUsageRow) {
        self.cpu_usages.push(row);
    }

    pub fn push_diskio(&mut self, row: DiskioRow) {
        self.diskios.push(row);
    }

    pub fn intern_string(&mut self, data: impl AsRef<str>) -> u64 {
        let data = data.as_ref();
        if let Some(id) = self.data_dict_index.get(data) {
            return *id;
        }
        let id = self.data_dict.len() as u64;
        let owned = data.to_string();
        self.data_dict.push(DataDictRow {
            id,
            data: owned.clone(),
        });
        self.data_dict_index.insert(owned, id);
        id
    }

    pub fn next_argset_id(&self) -> u64 {
        self.args
            .last()
            .map(|row| row.argset.saturating_add(1))
            .unwrap_or(0)
    }

    pub fn push_arg(&mut self, key: u64, datatype: u32, value: i64, argset: u64) {
        self.args.push(ArgsRow {
            id: self.args.len() as u64,
            key,
            datatype,
            value,
            argset,
        });
    }

    pub fn next_callstack_id(&self) -> u64 {
        self.callstacks.len() as u64
    }

    pub fn push_callstack(&mut self, row: CallstackRow) -> usize {
        if let Some(cat) = &row.cat {
            self.intern_string(cat);
        }
        if let Some(name) = &row.name {
            self.intern_string(name);
        }
        if let Some(flag) = &row.flag {
            self.intern_string(flag);
        }
        if let Some(trace_level) = &row.trace_level {
            self.intern_string(trace_level);
        }
        if let Some(trace_tag) = &row.trace_tag {
            self.intern_string(trace_tag);
        }
        if let Some(custom_category) = &row.custom_category {
            self.intern_string(custom_category);
        }
        self.callstacks.push(row);
        self.callstacks.len() - 1
    }

    pub fn callstack_mut(&mut self, row_id: usize) -> Option<&mut CallstackRow> {
        self.callstacks.get_mut(row_id)
    }

    pub fn callstack_id_at(&self, row_id: usize) -> Option<u64> {
        self.callstacks.get(row_id).map(|row| row.id)
    }

    pub fn push_process_measure(&mut self, row: MeasureRow) -> usize {
        self.process_measures.push(row);
        self.process_measures.len() - 1
    }

    pub fn process_measure_mut(&mut self, row_id: usize) -> Option<&mut MeasureRow> {
        self.process_measures.get_mut(row_id)
    }

    pub fn push_process_measure_filter(&mut self, row: ProcessMeasureFilterRow) {
        self.intern_string(&row.name);
        self.process_measure_filters.push(row);
    }

    pub fn push_sys_mem_measure(&mut self, row: MeasureRow) -> usize {
        self.sys_mem_measures.push(row);
        self.sys_mem_measures.len() - 1
    }

    pub fn sys_mem_measure_mut(&mut self, row_id: usize) -> Option<&mut MeasureRow> {
        self.sys_mem_measures.get_mut(row_id)
    }

    pub fn push_sys_event_filter(&mut self, row: SysEventFilterRow) {
        self.intern_string(&row.filter_type);
        self.intern_string(&row.name);
        self.sys_event_filters.push(row);
    }

    pub fn push_live_process(&mut self, row: LiveProcessRow) {
        self.live_processes.push(row);
    }

    pub fn push_js_heap_file(&mut self, row: JsHeapFilesRow) {
        self.js_heap_files.push(row);
    }

    pub fn push_js_heap_info(&mut self, row: JsHeapInfoRow) {
        self.js_heap_info.push(row);
    }

    pub fn push_js_heap_node(&mut self, row: JsHeapNodesRow) {
        self.js_heap_nodes.push(row);
    }

    pub fn js_heap_node_self_sizes(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.js_heap_nodes
            .iter()
            .map(|row| (row.file_id, row.self_size))
    }

    pub fn push_js_heap_edge(&mut self, row: JsHeapEdgesRow) {
        self.js_heap_edges.push(row);
    }

    pub fn push_js_heap_string(&mut self, row: JsHeapStringRow) {
        self.js_heap_string.push(row);
    }

    pub fn push_js_heap_location(&mut self, row: JsHeapLocationRow) {
        self.js_heap_location.push(row);
    }

    pub fn push_js_heap_sample(&mut self, row: JsHeapSampleRow) {
        self.js_heap_sample.push(row);
    }

    pub fn push_js_heap_trace_function_info(&mut self, row: JsHeapTraceFunctionInfoRow) {
        self.js_heap_trace_function_info.push(row);
    }

    pub fn push_js_heap_trace_node(&mut self, row: JsHeapTraceNodeRow) {
        self.js_heap_trace_node.push(row);
    }

    pub fn push_js_config(&mut self, row: JsConfigRow) {
        self.js_config.push(row);
    }

    pub fn push_js_cpu_profiler_node(&mut self, row: JsCpuProfilerNodeRow) {
        self.js_cpu_profiler_node.push(row);
    }

    pub fn next_js_cpu_profiler_sample_id(&self) -> u64 {
        self.js_cpu_profiler_sample.len() as u64
    }

    pub fn push_js_cpu_profiler_sample(&mut self, row: JsCpuProfilerSampleRow) {
        self.js_cpu_profiler_sample.push(row);
    }

    pub fn push_log(&mut self, row: LogRow) {
        self.logs.push(row);
    }

    pub fn push_hisysevent_all_event(&mut self, row: HiSysEventAllRow) {
        self.hisysevent_all_events.push(row);
    }

    pub fn next_hisysevent_measure_id(&self) -> u64 {
        self.hisysevent_measures.len() as u64
    }

    pub fn push_hisysevent_measure(&mut self, row: HiSysEventMeasureRow) {
        self.hisysevent_measures.push(row);
    }

    pub fn push_perf_report(&mut self, row: PerfReportRow) {
        self.perf_reports.push(row);
    }

    pub fn next_perf_file_id(&self) -> u64 {
        self.perf_files.len() as u64
    }

    pub fn push_perf_file(&mut self, row: PerfFilesRow) {
        self.perf_files.push(row);
    }

    pub fn next_perf_thread_id(&self) -> u64 {
        self.perf_threads.len() as u64
    }

    pub fn push_perf_thread(&mut self, row: PerfThreadRow) {
        self.perf_threads.push(row);
    }

    pub fn next_perf_sample_id(&self) -> u64 {
        self.perf_samples.len() as u64
    }

    pub fn push_perf_sample(&mut self, row: PerfSampleRow) {
        self.perf_samples.push(row);
    }

    pub fn next_perf_callchain_id(&self) -> u64 {
        self.perf_callchains.len() as u64
    }

    pub fn push_perf_callchain(&mut self, row: PerfCallchainRow) {
        self.perf_callchains.push(row);
    }

    pub fn finish(
        self,
        trace_id: String,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        clock_domain: String,
    ) -> ModelResult<crate::TraceTables> {
        Ok(crate::TraceTables {
            trace_metadata: metadata_batch(self.metadata)?,
            trace_bounds: trace_bounds_batch(trace_id, start_ts, end_ts, clock_domain)?,
            process: process_batch(self.processes)?,
            thread: thread_batch(self.threads)?,
            sched_slice: sched_slice_batch(self.sched_slices)?,
            thread_state: thread_state_batch(self.thread_states)?,
            raw_event: raw_event_batch(self.raw_events)?,
            raw: raw_batch(self.raw_rows)?,
            instant: instant_batch(self.instants)?,
            irq: irq_batch(self.irqs)?,
            measure: measure_batch(self.measures)?,
            measure_filter: measure_filter_batch(self.measure_filters)?,
            cpu_measure_filter: cpu_measure_filter_batch(self.cpu_measure_filters)?,
            symbols: symbols_batch(self.symbols)?,
            dma_fence: dma_fence_batch(self.dma_fences)?,
            cpu_usage: cpu_usage_batch(self.cpu_usages)?,
            diskio: diskio_batch(self.diskios)?,
            data_dict: data_dict_batch(self.data_dict)?,
            args: args_batch(self.args)?,
            callstack: callstack_batch(self.callstacks)?,
            process_measure: process_measure_batch(self.process_measures)?,
            process_measure_filter: process_measure_filter_batch(self.process_measure_filters)?,
            sys_mem_measure: sys_mem_measure_batch(self.sys_mem_measures)?,
            sys_event_filter: sys_event_filter_batch(self.sys_event_filters)?,
            live_process: live_process_batch(self.live_processes)?,
            js_heap_files: js_heap_files_batch(self.js_heap_files)?,
            js_heap_info: js_heap_info_batch(self.js_heap_info)?,
            js_heap_nodes: js_heap_nodes_batch(self.js_heap_nodes)?,
            js_heap_edges: js_heap_edges_batch(self.js_heap_edges)?,
            js_heap_string: js_heap_string_batch(self.js_heap_string)?,
            js_heap_location: js_heap_location_batch(self.js_heap_location)?,
            js_heap_sample: js_heap_sample_batch(self.js_heap_sample)?,
            js_heap_trace_function_info: js_heap_trace_function_info_batch(
                self.js_heap_trace_function_info,
            )?,
            js_heap_trace_node: js_heap_trace_node_batch(self.js_heap_trace_node)?,
            js_config: js_config_batch(self.js_config)?,
            js_cpu_profiler_node: js_cpu_profiler_node_batch(self.js_cpu_profiler_node)?,
            js_cpu_profiler_sample: js_cpu_profiler_sample_batch(self.js_cpu_profiler_sample)?,
            log: log_batch(self.logs)?,
            hisysevent_all_event: hisysevent_all_event_batch(self.hisysevent_all_events)?,
            hisysevent_measure: hisysevent_measure_batch(self.hisysevent_measures)?,
            perf_report: perf_report_batch(self.perf_reports)?,
            perf_files: perf_files_batch(self.perf_files)?,
            perf_thread: perf_thread_batch(self.perf_threads)?,
            perf_sample: perf_sample_batch(self.perf_samples)?,
            perf_callchain: perf_callchain_batch(self.perf_callchains)?,
        })
    }
}

fn metadata_batch(rows: Vec<(String, Option<String>)>) -> ModelResult<RecordBatch> {
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

fn trace_bounds_batch(
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

fn process_batch(rows: Vec<ProcessRow>) -> ModelResult<RecordBatch> {
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

fn thread_batch(rows: Vec<ThreadRow>) -> ModelResult<RecordBatch> {
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

fn sched_slice_batch(rows: Vec<SchedSliceRow>) -> ModelResult<RecordBatch> {
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

fn thread_state_batch(rows: Vec<ThreadStateRow>) -> ModelResult<RecordBatch> {
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

fn raw_event_batch(rows: Vec<RawEventRow>) -> ModelResult<RecordBatch> {
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

fn raw_batch(rows: Vec<RawRow>) -> ModelResult<RecordBatch> {
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

fn instant_batch(rows: Vec<InstantRow>) -> ModelResult<RecordBatch> {
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

fn irq_batch(rows: Vec<IrqRow>) -> ModelResult<RecordBatch> {
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

fn measure_batch(rows: Vec<MeasureRow>) -> ModelResult<RecordBatch> {
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

fn measure_filter_batch(rows: Vec<MeasureFilterRow>) -> ModelResult<RecordBatch> {
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

fn cpu_measure_filter_batch(rows: Vec<CpuMeasureFilterRow>) -> ModelResult<RecordBatch> {
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

fn symbols_batch(rows: Vec<SymbolsRow>) -> ModelResult<RecordBatch> {
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

fn dma_fence_batch(rows: Vec<DmaFenceRow>) -> ModelResult<RecordBatch> {
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

fn cpu_usage_batch(rows: Vec<CpuUsageRow>) -> ModelResult<RecordBatch> {
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

fn diskio_batch(rows: Vec<DiskioRow>) -> ModelResult<RecordBatch> {
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

fn data_dict_batch(rows: Vec<DataDictRow>) -> ModelResult<RecordBatch> {
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

fn args_batch(rows: Vec<ArgsRow>) -> ModelResult<RecordBatch> {
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

fn callstack_batch(rows: Vec<CallstackRow>) -> ModelResult<RecordBatch> {
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

fn process_measure_batch(rows: Vec<MeasureRow>) -> ModelResult<RecordBatch> {
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

fn sys_mem_measure_batch(rows: Vec<MeasureRow>) -> ModelResult<RecordBatch> {
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

fn process_measure_filter_batch(rows: Vec<ProcessMeasureFilterRow>) -> ModelResult<RecordBatch> {
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

fn sys_event_filter_batch(rows: Vec<SysEventFilterRow>) -> ModelResult<RecordBatch> {
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

fn live_process_batch(rows: Vec<LiveProcessRow>) -> ModelResult<RecordBatch> {
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

fn js_heap_files_batch(rows: Vec<JsHeapFilesRow>) -> ModelResult<RecordBatch> {
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

fn js_heap_info_batch(rows: Vec<JsHeapInfoRow>) -> ModelResult<RecordBatch> {
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

fn js_heap_nodes_batch(rows: Vec<JsHeapNodesRow>) -> ModelResult<RecordBatch> {
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

fn js_heap_edges_batch(rows: Vec<JsHeapEdgesRow>) -> ModelResult<RecordBatch> {
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

fn js_heap_string_batch(rows: Vec<JsHeapStringRow>) -> ModelResult<RecordBatch> {
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

fn js_heap_location_batch(rows: Vec<JsHeapLocationRow>) -> ModelResult<RecordBatch> {
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

fn js_heap_sample_batch(rows: Vec<JsHeapSampleRow>) -> ModelResult<RecordBatch> {
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

fn js_heap_trace_function_info_batch(
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

fn js_heap_trace_node_batch(rows: Vec<JsHeapTraceNodeRow>) -> ModelResult<RecordBatch> {
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

fn js_config_batch(rows: Vec<JsConfigRow>) -> ModelResult<RecordBatch> {
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

fn js_cpu_profiler_node_batch(rows: Vec<JsCpuProfilerNodeRow>) -> ModelResult<RecordBatch> {
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

fn js_cpu_profiler_sample_batch(rows: Vec<JsCpuProfilerSampleRow>) -> ModelResult<RecordBatch> {
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

fn log_batch(rows: Vec<LogRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        log_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.seq).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.pid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.tid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.level.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.tag.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.context.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.origints).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

fn hisysevent_all_event_batch(rows: Vec<HiSysEventAllRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        hisysevent_all_event_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.domain.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.event_name.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.event_type).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.time_zone.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.pid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.tid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.uid).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.level.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.tag.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.event_id.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.seq).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.info.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.contents.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
        ],
    )
}

fn hisysevent_measure_batch(rows: Vec<HiSysEventMeasureRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        hisysevent_measure_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.serial).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.ts).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.name.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int32Array::from(
                rows.iter().map(|r| r.value_type).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Float64Array::from(
                rows.iter().map(|r| r.int_value).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.string_value.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
        ],
    )
}

fn perf_report_batch(rows: Vec<PerfReportRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        perf_report_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.report_type.as_str())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.report_value.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
        ],
    )
}

fn perf_files_batch(rows: Vec<PerfFilesRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        perf_files_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.file_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.serial_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.symbol.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.path.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
        ],
    )
}

fn perf_thread_batch(rows: Vec<PerfThreadRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        perf_thread_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.thread_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.process_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.thread_name.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
        ],
    )
}

fn perf_sample_batch(rows: Vec<PerfSampleRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        perf_sample_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.callchain_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.timestamp).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.thread_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.event_count).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.event_type_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(Int64Array::from(
                rows.iter().map(|r| r.timestamp_trace).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.cpu_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.thread_state.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
        ],
    )
}

fn perf_callchain_batch(rows: Vec<PerfCallchainRow>) -> ModelResult<RecordBatch> {
    RecordBatch::try_new(
        perf_callchain_schema(),
        vec![
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.callchain_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.depth).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.ip).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.vaddr_in_file).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.offset_to_vaddr).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.file_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.symbol_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.name.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.source_file_id.as_deref())
                    .collect::<Vec<Option<&str>>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|r| r.line_number).collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_sched_slice_batch() {
        let mut builder = TraceTableBuilder::default();
        builder.push_sched_slice(SchedSliceRow {
            cpu: 0,
            utid: 1,
            ts: 100,
            dur: Some(50),
            priority: Some(120),
            end_state: Some("S".to_string()),
        });

        let tables = builder
            .finish(
                "test".to_string(),
                Some(100),
                Some(150),
                "boottime".to_string(),
            )
            .unwrap();

        assert_eq!(tables.sched_slice.num_rows(), 1);
        assert_eq!(tables.sched_slice.num_columns(), 6);
        assert_eq!(tables.log.num_rows(), 0);
    }
}
