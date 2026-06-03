use super::batches::*;
use super::rows::*;
use super::ModelResult;
use std::collections::HashMap;

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
}

impl TraceTableBuilder {
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
        })
    }
}
