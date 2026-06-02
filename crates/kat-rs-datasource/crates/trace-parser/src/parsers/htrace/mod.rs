use crate::plugins::{arkts, memory, process, shared};
use crate::TraceEngineError;
use crate::{HarmonyTraceParser, ParseResult};
use prost::Message;
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use trace_model::{
    CallstackRow, CpuMeasureFilterRow, CpuUsageRow, DiskioRow, DmaFenceRow, InstantRow, IrqRow,
    MeasureFilterRow, MeasureRow, ParsedTrace, ProcessRow, RawEventRow, RawRow, SchedSliceRow,
    SymbolsRow, ThreadRow, ThreadStateRow, TraceTableBuilder,
};

mod proto;
pub use proto::*;

const PROFILER_HEADER_SIZE: usize = 1024;
const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
const HIPROFILER_PROTOBUF_BIN: u32 = 0;
const SEGMENT_LENGTH_SIZE: usize = 4;
const TS_CLOCK_BOOTTIME: i32 = 1;
const TS_CLOCK_REALTIME: i32 = 2;
const TS_CLOCK_REALTIME_COARSE: i32 = 3;
const TS_CLOCK_MONOTONIC: i32 = 4;
const TS_CLOCK_MONOTONIC_COARSE: i32 = 5;
const TS_CLOCK_MONOTONIC_RAW: i32 = 6;
const ARG_DATATYPE_INT: u32 = 0;
const ARG_DATATYPE_STRING: u32 = 1;
const ARG_DATATYPE_BOOLEAN: u32 = 3;
const BINDER_ONEWAY_FLAG: u32 = 0x01;
const BINDER_ROOT_OBJECT_FLAG: u32 = 0x04;
const BINDER_STATUS_CODE_FLAG: u32 = 0x08;
const BINDER_ACCEPT_FDS_FLAG: u32 = 0x10;

#[derive(Debug, Clone)]
struct ThreadInfo {
    utid: u32,
    tid: u32,
    upid: u32,
    name: Option<String>,
    end_ts: Option<i64>,
}

#[derive(Debug, Clone)]
struct ProcessInfoState {
    upid: u32,
    pid: u32,
    name: Option<String>,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
}

#[derive(Debug, Clone)]
struct OpenSchedSlice {
    row_id: usize,
    ts: i64,
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
struct IrqKey {
    cpu: u32,
    cat: &'static str,
    callid: i32,
}

struct TimedFtraceEvent {
    ts: i64,
    order: usize,
    cpu: u32,
    event: FtraceEvent,
}

#[derive(Default)]
struct BinderTraceState {
    sync_transaction_by_id: HashMap<i32, PendingBinderTransaction>,
    reply_by_tid: HashMap<u32, usize>,
    reply_destination_by_tid: HashMap<u32, u32>,
    reply_waiting_by_id: HashSet<i32>,
    async_transaction_args: HashMap<i32, u64>,
    lock_wait_by_tid: HashMap<u32, usize>,
    lock_held_by_tid: HashMap<u32, usize>,
}

#[derive(Debug, Clone, Copy)]
struct PendingBinderTransaction {
    row_id: usize,
    sender_tid: u32,
}

#[derive(Default)]
pub struct HtraceParser {
    tables: TraceTableBuilder,
    processes_by_pid: BTreeMap<u32, ProcessInfoState>,
    threads_by_tid: BTreeMap<u32, ThreadInfo>,
    cpu_running: HashMap<u32, OpenSchedSlice>,
    thread_state_open: HashMap<u32, usize>,
    pending_wakeup_by_tid: HashMap<u32, i64>,
    open_irqs: HashMap<IrqKey, Vec<usize>>,
    measure_filters: HashMap<(String, String, Option<u32>), u64>,
    open_measures: HashMap<u64, usize>,
    symbol_addrs: HashSet<u64>,
    symbols_by_addr: HashMap<u64, String>,
    pending_cpu_usage: Option<CpuUsageRow>,
    memory_state: memory::MemoryMeasureState,
    live_process_state: process::LiveProcessState,
    arkts_state: arkts::ArkTsState,
    shared_trace: shared::SharedTraceState,
    binder_state: BinderTraceState,
    workqueue_stack_by_tid: HashMap<u32, Vec<usize>>,
    pending_ftrace_events: Vec<TimedFtraceEvent>,
    next_ftrace_order: usize,
    next_id: u32,
    next_measure_filter_id: u64,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    clock_domain: String,
    clock_offsets: HashMap<(i32, i32), BTreeMap<u64, i128>>,
    input_hash: u64,
}

impl HtraceParser {
    pub fn new() -> Self {
        Self {
            clock_domain: "unknown".to_string(),
            ..Self::default()
        }
    }

    fn reset_for_input(&mut self, bytes: &[u8]) {
        *self = Self::new();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        self.input_hash = hasher.finish();
        self.tables.push_metadata("parser", Some("trace-parser"));
        self.tables.push_metadata("parser_version", Some("0.1.0"));
        self.tables.push_metadata("source_format", Some("htrace"));
    }

    fn parse_framed_file(&mut self, bytes: &[u8]) -> ParseResult<()> {
        let mut offset = 0usize;
        while offset < bytes.len() {
            if bytes.len() - offset < PROFILER_HEADER_SIZE {
                return Err(TraceEngineError::Parse(format!(
                    "truncated profiler header at byte {offset}"
                )));
            }

            let header = &bytes[offset..offset + PROFILER_HEADER_SIZE];
            let magic = read_u64_le(header, 0)?;
            if magic != PROFILER_HEADER_MAGIC {
                return Err(TraceEngineError::Parse(format!(
                    "invalid profiler header magic at byte {offset}: 0x{magic:x}"
                )));
            }

            self.add_header_clock_snapshot(header)?;
            let section_len = read_u64_le(header, 8)? as usize;
            let data_type = read_u32_le(header, 56)?;
            if section_len < PROFILER_HEADER_SIZE || offset + section_len > bytes.len() {
                return Err(TraceEngineError::Parse(format!(
                    "invalid profiler section length {section_len} at byte {offset}"
                )));
            }

            let section_end = offset + section_len;
            offset += PROFILER_HEADER_SIZE;

            if data_type != HIPROFILER_PROTOBUF_BIN {
                self.tables.push_raw_event(RawEventRow {
                    ts: None,
                    cpu: None,
                    tid: None,
                    event_name: "unsupported_profiler_section".to_string(),
                    payload_json: Some(json!({ "data_type": data_type }).to_string()),
                });
                offset = section_end;
                continue;
            }

            while offset < section_end {
                if section_end - offset < SEGMENT_LENGTH_SIZE {
                    return Err(TraceEngineError::Parse(format!(
                        "truncated segment length at byte {offset}"
                    )));
                }
                let len = read_u32_le(bytes, offset)? as usize;
                offset += SEGMENT_LENGTH_SIZE;
                if offset + len > section_end {
                    return Err(TraceEngineError::Parse(format!(
                        "segment length {len} exceeds section boundary at byte {offset}"
                    )));
                }
                let segment = &bytes[offset..offset + len];
                self.parse_profiler_segment(segment)?;
                offset += len;
            }
        }
        Ok(())
    }

    fn parse_len_prefixed_segments(&mut self, bytes: &[u8]) -> ParseResult<()> {
        let mut offset = 0usize;
        while offset < bytes.len() {
            if bytes.len() - offset < SEGMENT_LENGTH_SIZE {
                return Err(TraceEngineError::Parse(format!(
                    "truncated segment length at byte {offset}"
                )));
            }
            let len = read_u32_le(bytes, offset)? as usize;
            offset += SEGMENT_LENGTH_SIZE;
            if offset + len > bytes.len() {
                return Err(TraceEngineError::Parse(format!(
                    "segment length {len} exceeds input at byte {offset}"
                )));
            }
            self.parse_profiler_segment(&bytes[offset..offset + len])?;
            offset += len;
        }
        Ok(())
    }

    fn parse_profiler_segment(&mut self, segment: &[u8]) -> ParseResult<()> {
        let plugin = ProfilerPluginData::decode(segment).map_err(|err| {
            TraceEngineError::Parse(format!("failed to decode ProfilerPluginData: {err}"))
        })?;

        match plugin.name.as_str() {
            "ftrace-plugin" | "/data/local/tmp/libftrace_plugin.z.so" => {
                self.parse_ftrace_plugin(&plugin)
            }
            "cpu-plugin" => self.parse_cpu_plugin(&plugin),
            "diskio-plugin" => self.parse_diskio_plugin(&plugin),
            "memory-plugin" => self.parse_memory_plugin(&plugin),
            "process-plugin" => self.parse_process_plugin(&plugin),
            "arkts-plugin_config" => {
                arkts::parse_config(&plugin.data, &mut self.tables, &mut self.arkts_state)
            }
            "arkts-plugin" => {
                let ts = self.plugin_realtime_ts(&plugin);
                let monotonic_offsets = self
                    .clock_offsets
                    .get(&(TS_CLOCK_MONOTONIC, TS_CLOCK_BOOTTIME))
                    .cloned();
                arkts::parse_arkts_plugin(
                    &plugin.data,
                    ts,
                    &mut self.tables,
                    &mut self.arkts_state,
                    |src_ts| convert_clock_with_offsets(monotonic_offsets.as_ref(), src_ts),
                )
            }
            _ => {
                self.tables.push_raw_event(RawEventRow {
                    ts: plugin_outer_ts(&plugin),
                    cpu: None,
                    tid: None,
                    event_name: plugin.name,
                    payload_json: Some(
                        json!({
                            "status": plugin.status,
                            "clock_id": plugin.clock_id,
                            "data_len": plugin.data.len()
                        })
                        .to_string(),
                    ),
                });
                Ok(())
            }
        }
    }

    fn parse_ftrace_plugin(&mut self, plugin: &ProfilerPluginData) -> ParseResult<()> {
        let trace = TracePluginResult::decode(plugin.data.as_slice()).map_err(|err| {
            TraceEngineError::Parse(format!("failed to decode TracePluginResult: {err}"))
        })?;

        if !self.has_clock_snapshot() {
            let snapshots = trace
                .clocks_detail
                .iter()
                .filter_map(|clock| {
                    let time = clock.time.as_ref()?;
                    let ts = u64::from(time.tv_sec)
                        .saturating_mul(1_000_000_000)
                        .saturating_add(u64::from(time.tv_nsec));
                    (ts != 0).then_some((clock.id, ts))
                })
                .collect::<Vec<_>>();
            self.add_clock_snapshot(&snapshots);
        }

        for stats in trace.ftrace_cpu_stats {
            match stats.trace_clock.as_str() {
                "boot" => self.clock_domain = "boottime".to_string(),
                "mono" => self.clock_domain = "monotonic".to_string(),
                clock if !clock.is_empty() => self.clock_domain = clock.to_string(),
                _ => {}
            }
        }

        for symbol in trace.symbols_detail {
            let symbol_name = symbol.symbol_name;
            self.symbols_by_addr
                .entry(symbol.symbol_addr)
                .or_insert_with(|| symbol_name.clone());
            if self.symbol_addrs.insert(symbol.symbol_addr) {
                self.tables.intern_string(&symbol_name);
                self.tables.push_symbol(SymbolsRow {
                    id: self.tables.next_symbol_id(),
                    funcname: symbol_name,
                    addr: symbol.symbol_addr,
                });
            }
        }

        for cpu_detail in trace.ftrace_cpu_detail {
            for event in cpu_detail.event {
                let ts = event.timestamp as i64;
                self.pending_ftrace_events.push(TimedFtraceEvent {
                    ts,
                    order: self.next_ftrace_order,
                    cpu: cpu_detail.cpu,
                    event,
                });
                self.next_ftrace_order += 1;
            }
        }

        Ok(())
    }

    fn parse_cpu_plugin(&mut self, plugin: &ProfilerPluginData) -> ParseResult<()> {
        let data = CpuData::decode(plugin.data.as_slice())
            .map_err(|err| TraceEngineError::Parse(format!("failed to decode CpuData: {err}")))?;
        let ts = data
            .cpu_usage_info
            .as_ref()
            .and_then(|info| info.timestamp.as_ref())
            .map(sample_ts_to_ns)
            .or_else(|| plugin_outer_ts(plugin));
        let Some(ts) = ts else {
            return Ok(());
        };

        let current = CpuUsageRow {
            ts,
            dur: None,
            total_load: data.total_load,
            user_load: data.user_load,
            system_load: data.sys_load,
            process_num: data.process_num,
        };

        if let Some(mut previous) = self.pending_cpu_usage.replace(current) {
            previous.dur = Some(ts.saturating_sub(previous.ts));
            self.tables.push_cpu_usage(previous);
        }

        Ok(())
    }

    fn parse_diskio_plugin(&mut self, plugin: &ProfilerPluginData) -> ParseResult<()> {
        let data = DiskioData::decode(plugin.data.as_slice()).map_err(|err| {
            TraceEngineError::Parse(format!("failed to decode DiskioData: {err}"))
        })?;
        let Some(prev_ts) = data.prev_timestamp.as_ref().map(collect_ts_to_ns) else {
            return Ok(());
        };
        let Some(ts) = data.timestamp.as_ref().map(collect_ts_to_ns) else {
            return Ok(());
        };
        if prev_ts == 0 || ts <= prev_ts {
            return Ok(());
        }

        let dur = ts - prev_ts;
        let rd_delta = data.rd_sectors_kb.saturating_sub(data.prev_rd_sectors_kb);
        let wr_delta = data.wr_sectors_kb.saturating_sub(data.prev_wr_sectors_kb);
        let scale = 1_000_000_000.0 / dur as f64;
        self.tables.push_diskio(DiskioRow {
            ts: prev_ts,
            dur: Some(dur),
            rd: data.rd_sectors_kb,
            wr: data.wr_sectors_kb,
            rd_speed: rd_delta as f64 * scale,
            wr_speed: wr_delta as f64 * scale,
            rd_count: data.rd_sectors_kb.saturating_mul(2),
            wr_count: data.wr_sectors_kb.saturating_mul(2),
            rd_count_speed: 0.0,
            wr_count_speed: 0.0,
        });

        Ok(())
    }

    fn parse_memory_plugin(&mut self, plugin: &ProfilerPluginData) -> ParseResult<()> {
        let ts = self.plugin_realtime_ts(plugin);
        let mut tables = std::mem::take(&mut self.tables);
        let mut memory_state = std::mem::take(&mut self.memory_state);
        let result = memory::parse_memory_plugin(
            &plugin.data,
            ts,
            &mut tables,
            &mut memory_state,
            |sample_ts, pid, name| self.get_or_create_process(sample_ts, pid, name),
        );
        self.tables = tables;
        self.memory_state = memory_state;
        result
    }

    fn parse_process_plugin(&mut self, plugin: &ProfilerPluginData) -> ParseResult<()> {
        process::parse_process_plugin(
            &plugin.data,
            self.plugin_realtime_ts(plugin),
            &mut self.live_process_state,
        )
    }

    fn on_print_event(&mut self, ts: i64, tid: u32, _tgid: u32, _comm: &str, print: PrintFormat) {
        let Some(marker) = shared::parse_trace_marker(&print.buf) else {
            return;
        };
        match marker {
            shared::TraceMarker::Counter {
                callid,
                name,
                value,
            } => {
                let ipid = self.get_or_create_process(ts, callid, None);
                memory::append_process_metric(
                    &mut self.tables,
                    &mut self.memory_state,
                    ts,
                    ipid,
                    &name,
                    value,
                );
            }
            marker => shared::handle_trace_marker(
                &mut self.tables,
                &mut self.shared_trace,
                ts,
                tid,
                marker,
            ),
        }
    }

    fn on_workqueue_execute_start(
        &mut self,
        ts: i64,
        tid: u32,
        comm: &str,
        workqueue: WorkqueueExecuteStartFormat,
    ) {
        let utid = self.get_or_create_thread(ts, tid, non_empty_str(comm));
        let name = self
            .symbols_by_addr
            .get(&workqueue.function)
            .cloned()
            .unwrap_or_else(|| format!("0x{:x}", workqueue.function));
        let parent_id = self
            .workqueue_stack_by_tid
            .get(&utid)
            .and_then(|stack| stack.last())
            .and_then(|row_id| self.tables.callstack_id_at(*row_id));
        let depth = self
            .workqueue_stack_by_tid
            .get(&utid)
            .map(|stack| stack.len() as u32)
            .unwrap_or_default();
        let row_id = self.push_callstack_slice(
            ts,
            utid,
            Some("workqueue"),
            &name,
            Some(depth),
            parent_id,
            None,
            None,
        );
        self.workqueue_stack_by_tid
            .entry(utid)
            .or_default()
            .push(row_id);
    }

    fn on_workqueue_execute_end(&mut self, ts: i64, tid: u32) {
        let utid = self.get_or_create_thread(ts, tid, None);
        if let Some(stack) = self.workqueue_stack_by_tid.get_mut(&utid) {
            if let Some(row_id) = stack.pop() {
                self.close_callstack_row(row_id, ts);
            }
        }
    }

    fn on_oom_score_adj_update(&mut self, ts: i64, oom: OomScoreAdjUpdateFormat) {
        let pid = u32::try_from(oom.pid).unwrap_or_default();
        let ipid = self.get_or_create_process(ts, pid, non_empty_str(&oom.comm));
        memory::append_process_metric(
            &mut self.tables,
            &mut self.memory_state,
            ts,
            ipid,
            "oom_score_adj",
            i64::from(oom.oom_score_adj),
        );
    }

    fn on_binder_transaction(&mut self, ts: i64, tid: u32, transaction: BinderTransactionFormat) {
        if transaction.reply == 1 {
            if let Some(row_id) = self.binder_state.reply_by_tid.remove(&tid) {
                if self
                    .binder_state
                    .reply_destination_by_tid
                    .get(&tid)
                    .copied()
                    == Some(u32::try_from(transaction.to_thread).unwrap_or_default())
                {
                    let dest_tid = u32::try_from(transaction.to_thread).unwrap_or_default();
                    let dest_name = self.thread_name_for_tid(dest_tid);
                    self.append_destination_thread_args(row_id, dest_tid, dest_name.as_deref());
                    self.binder_state.reply_destination_by_tid.remove(&tid);
                }
                let argset = self.ensure_callstack_argset(row_id);
                self.append_binder_transaction_args(argset, tid, &transaction);
                self.close_callstack_row(row_id, ts);
            }
            self.binder_state
                .reply_waiting_by_id
                .insert(transaction.debug_id);
            return;
        }

        let argset = self.binder_transaction_argset(tid, &transaction);
        if (transaction.flags & BINDER_ONEWAY_FLAG) == BINDER_ONEWAY_FLAG {
            let row_id =
                self.push_binder_row(ts, tid, "binder transaction async", Some(0), Some(argset));
            self.binder_state
                .async_transaction_args
                .insert(transaction.debug_id, argset);
            self.binder_state.sync_transaction_by_id.insert(
                transaction.debug_id,
                PendingBinderTransaction {
                    row_id,
                    sender_tid: tid,
                },
            );
        } else {
            let row_id = self.push_binder_row(ts, tid, "binder transaction", None, Some(argset));
            self.binder_state.sync_transaction_by_id.insert(
                transaction.debug_id,
                PendingBinderTransaction {
                    row_id,
                    sender_tid: tid,
                },
            );
        }
    }

    fn on_binder_transaction_received(
        &mut self,
        ts: i64,
        tid: u32,
        comm: &str,
        received: BinderTransactionReceivedFormat,
    ) {
        let pending = self
            .binder_state
            .sync_transaction_by_id
            .remove(&received.debug_id);
        if let Some(pending) = pending {
            self.close_callstack_row(pending.row_id, ts);
        }

        if let Some(argset) = self
            .binder_state
            .async_transaction_args
            .remove(&received.debug_id)
        {
            self.push_binder_row(ts, tid, "binder async rcv", Some(0), Some(argset));
            return;
        }

        if self
            .binder_state
            .reply_waiting_by_id
            .remove(&received.debug_id)
        {
            return;
        }

        let row_id = self.push_binder_row(ts, tid, "binder reply", None, None);
        let dest_name = self
            .thread_name_for_tid(tid)
            .or_else(|| non_empty_str(comm).map(ToOwned::to_owned));
        if let Some(pending) = pending {
            let reply_slice_id = self.tables.callstack_id_at(row_id).unwrap_or_default() as i64;
            self.append_int_arg_to_callstack(
                pending.row_id,
                "destination slice id",
                reply_slice_id,
            );
            self.append_destination_thread_args(pending.row_id, tid, dest_name.as_deref());
            if let Some(trans_slice_id) = self.tables.callstack_id_at(pending.row_id) {
                self.append_int_arg_to_callstack(
                    row_id,
                    "destination slice id",
                    trans_slice_id as i64,
                );
            }
            self.binder_state
                .reply_destination_by_tid
                .insert(tid, pending.sender_tid);
        }
        self.binder_state.reply_by_tid.insert(tid, row_id);
    }

    fn on_binder_transaction_alloc_buf(&mut self, alloc: BinderTransactionAllocBufFormat) {
        let Some(pending) = self
            .binder_state
            .sync_transaction_by_id
            .get(&alloc.debug_id)
            .copied()
        else {
            return;
        };
        self.append_int_arg_to_callstack(pending.row_id, "data size", alloc.data_size as i64);
        self.append_int_arg_to_callstack(pending.row_id, "offsets size", alloc.offsets_size as i64);
    }

    fn on_binder_lock(&mut self, ts: i64, tid: u32) {
        let row_id = self.push_binder_row(ts, tid, "binder lock waiting", None, None);
        self.binder_state.lock_wait_by_tid.insert(tid, row_id);
    }

    fn on_binder_locked(&mut self, ts: i64, tid: u32) {
        if let Some(row_id) = self.binder_state.lock_wait_by_tid.remove(&tid) {
            self.close_callstack_row(row_id, ts);
        }
        let row_id = self.push_binder_row(ts, tid, "binder lock held", None, None);
        self.binder_state.lock_held_by_tid.insert(tid, row_id);
    }

    fn on_binder_unlock(&mut self, ts: i64, tid: u32) {
        if let Some(row_id) = self.binder_state.lock_held_by_tid.remove(&tid) {
            self.close_callstack_row(row_id, ts);
        }
    }

    fn push_binder_row(
        &mut self,
        ts: i64,
        tid: u32,
        name: &str,
        dur: Option<i64>,
        argsetid: Option<u64>,
    ) -> usize {
        let utid = self.get_or_create_thread(ts, tid, None);
        self.push_callstack_slice(ts, utid, Some("binder"), name, Some(0), None, dur, argsetid)
    }

    fn push_callstack_slice(
        &mut self,
        ts: i64,
        callid: u32,
        cat: Option<&str>,
        name: &str,
        depth: Option<u32>,
        parent_id: Option<u64>,
        dur: Option<i64>,
        argsetid: Option<u64>,
    ) -> usize {
        self.tables.push_callstack(CallstackRow {
            id: self.tables.next_callstack_id(),
            ts,
            dur,
            callid: Some(callid),
            cat: cat.map(ToOwned::to_owned),
            name: Some(name.to_string()),
            depth,
            cookie: None,
            parent_id,
            argsetid,
            chain_id: None,
            span_id: None,
            parent_span_id: None,
            flag: None,
            trace_level: None,
            trace_tag: None,
            custom_category: None,
            custom_args: None,
            child_callid: None,
        })
    }

    fn close_callstack_row(&mut self, row_id: usize, ts: i64) {
        if let Some(row) = self.tables.callstack_mut(row_id) {
            row.dur = Some(ts.saturating_sub(row.ts));
        }
    }

    fn binder_transaction_argset(
        &mut self,
        tid: u32,
        transaction: &BinderTransactionFormat,
    ) -> u64 {
        let argset = self.tables.next_argset_id();
        self.append_binder_transaction_args(argset, tid, transaction);
        argset
    }

    fn append_binder_transaction_args(
        &mut self,
        argset: u64,
        tid: u32,
        transaction: &BinderTransactionFormat,
    ) {
        self.push_int_arg(argset, "transaction id", i64::from(transaction.debug_id));
        self.push_int_arg(
            argset,
            "destination node",
            i64::from(transaction.target_node),
        );
        self.push_int_arg(
            argset,
            "destination process",
            i64::from(transaction.to_proc),
        );
        self.push_bool_arg(argset, "reply transaction?", transaction.reply == 1);
        let flags_desc = binder_flags_desc(transaction.flags);
        self.push_string_arg(
            argset,
            "flags",
            &format!("0x{:x}{}", transaction.flags, flags_desc.trim_end()),
        );
        self.push_string_arg(
            argset,
            "code",
            &format!("0x{:x} Java Layer Dependent", transaction.code),
        );
        self.push_int_arg(argset, "calling tid", i64::from(tid));
    }

    fn append_int_arg_to_callstack(&mut self, row_id: usize, key: &str, value: i64) {
        let argset = self.ensure_callstack_argset(row_id);
        self.push_int_arg(argset, key, value);
    }

    fn append_string_arg_to_callstack(&mut self, row_id: usize, key: &str, value: &str) {
        let argset = self.ensure_callstack_argset(row_id);
        self.push_string_arg(argset, key, value);
    }

    fn append_destination_thread_args(
        &mut self,
        row_id: usize,
        destination_tid: u32,
        destination_name: Option<&str>,
    ) {
        self.append_int_arg_to_callstack(row_id, "destination thread", i64::from(destination_tid));
        self.append_string_arg_to_callstack(
            row_id,
            "destination name",
            destination_name.unwrap_or(""),
        );
    }

    fn ensure_callstack_argset(&mut self, row_id: usize) -> u64 {
        let existing_argset = self
            .tables
            .callstack_mut(row_id)
            .and_then(|row| row.argsetid);
        let argset = existing_argset.unwrap_or_else(|| self.tables.next_argset_id());
        if existing_argset.is_none() {
            if let Some(row) = self.tables.callstack_mut(row_id) {
                row.argsetid = Some(argset);
            }
        }
        argset
    }

    fn thread_name_for_tid(&self, tid: u32) -> Option<String> {
        self.threads_by_tid
            .get(&tid)
            .and_then(|info| non_empty_str(info.name.as_deref()?))
            .map(ToOwned::to_owned)
    }

    fn push_int_arg(&mut self, argset: u64, key: &str, value: i64) {
        let key_id = self.tables.intern_string(key);
        self.tables
            .push_arg(key_id, ARG_DATATYPE_INT, value, argset);
    }

    fn push_bool_arg(&mut self, argset: u64, key: &str, value: bool) {
        let key_id = self.tables.intern_string(key);
        self.tables.push_arg(
            key_id,
            ARG_DATATYPE_BOOLEAN,
            if value { 1 } else { 0 },
            argset,
        );
    }

    fn push_string_arg(&mut self, argset: u64, key: &str, value: &str) {
        let key_id = self.tables.intern_string(key);
        let value_id = self.tables.intern_string(value);
        self.tables
            .push_arg(key_id, ARG_DATATYPE_STRING, value_id as i64, argset);
    }

    fn add_header_clock_snapshot(&mut self, header: &[u8]) -> ParseResult<()> {
        if self.has_clock_snapshot() {
            return Ok(());
        }

        let snapshot = [
            (TS_CLOCK_BOOTTIME, 60usize),
            (TS_CLOCK_REALTIME, 68usize),
            (TS_CLOCK_REALTIME_COARSE, 76usize),
            (TS_CLOCK_MONOTONIC, 84usize),
            (TS_CLOCK_MONOTONIC_COARSE, 92usize),
            (TS_CLOCK_MONOTONIC_RAW, 100usize),
        ]
        .into_iter()
        .filter_map(|(clock_id, offset)| match read_u64_le(header, offset) {
            Ok(0) => None,
            Ok(ts) => Some(Ok((clock_id, ts))),
            Err(err) => Some(Err(err)),
        })
        .collect::<ParseResult<Vec<_>>>()?;
        self.add_clock_snapshot(&snapshot);
        Ok(())
    }

    fn has_clock_snapshot(&self) -> bool {
        !self.clock_offsets.is_empty()
    }

    fn add_clock_snapshot(&mut self, snapshot: &[(i32, u64)]) {
        if snapshot.len() < 2 {
            return;
        }
        for left in 0..snapshot.len() - 1 {
            for right in left + 1..snapshot.len() {
                let (src_clock, src_ts) = snapshot[left];
                let (dst_clock, dst_ts) = snapshot[right];
                self.add_convert_clock_map(src_clock, dst_clock, src_ts, dst_ts);
                self.add_convert_clock_map(dst_clock, src_clock, dst_ts, src_ts);
            }
        }
    }

    fn add_convert_clock_map(&mut self, src_clock: i32, dst_clock: i32, src_ts: u64, dst_ts: u64) {
        self.clock_offsets
            .entry((src_clock, dst_clock))
            .or_default()
            .insert(src_ts, dst_ts as i128 - src_ts as i128);
    }

    fn to_primary_trace_time(&self, src_clock: i32, src_ts: u64) -> u64 {
        if src_clock == TS_CLOCK_BOOTTIME {
            return src_ts;
        }
        self.convert_clock(src_clock, src_ts, TS_CLOCK_BOOTTIME)
    }

    fn convert_clock(&self, src_clock: i32, src_ts: u64, dst_clock: i32) -> u64 {
        if src_clock == dst_clock {
            return src_ts;
        }
        let Some(offsets) = self.clock_offsets.get(&(src_clock, dst_clock)) else {
            return src_ts;
        };
        let Some((_, offset)) = offsets.range(..=src_ts).next_back() else {
            return src_ts;
        };
        let converted = src_ts as i128 + *offset;
        converted.clamp(0, u64::MAX as i128) as u64
    }

    fn plugin_realtime_ts(&self, plugin: &ProfilerPluginData) -> Option<i64> {
        plugin_outer_ts(plugin)
            .map(|ts| self.to_primary_trace_time(TS_CLOCK_REALTIME, ts as u64) as i64)
    }

    fn process_pending_ftrace_events(&mut self) -> ParseResult<()> {
        let mut events = std::mem::take(&mut self.pending_ftrace_events);
        events.sort_by(|left, right| left.ts.cmp(&right.ts).then(left.order.cmp(&right.order)));

        for event in events {
            self.handle_ftrace_event(event.ts, event.cpu, event.event)?;
        }

        Ok(())
    }

    fn handle_ftrace_event(&mut self, ts: i64, cpu: u32, event: FtraceEvent) -> ParseResult<()> {
        self.observe_ts(ts);
        let tid = event.common_fields.as_ref().map(|f| sanitize_tid(f.pid));
        let event_tid = tid.unwrap_or_else(|| sanitize_tid(event.tgid));
        let event_tgid = sanitize_tid(event.tgid);
        let event_comm = event.comm.clone();
        if let Some(print) = event.print_format {
            self.on_print_event(ts, event_tid, event_tgid, &event_comm, print);
        } else if let Some(binder_transaction) = event.binder_transaction_format {
            self.on_binder_transaction(ts, event_tid, binder_transaction);
        } else if let Some(binder_received) = event.binder_transaction_received_format {
            self.on_binder_transaction_received(ts, event_tid, &event_comm, binder_received);
        } else if let Some(alloc_buf) = event.binder_transaction_alloc_buf_format {
            self.on_binder_transaction_alloc_buf(alloc_buf);
        } else if event.binder_lock_format.is_some() {
            self.on_binder_lock(ts, event_tid);
        } else if event.binder_locked_format.is_some() {
            self.on_binder_locked(ts, event_tid);
        } else if event.binder_unlock_format.is_some() {
            self.on_binder_unlock(ts, event_tid);
        } else if let Some(oom) = event.oom_score_adj_update_format {
            self.on_oom_score_adj_update(ts, oom);
        } else if let Some(workqueue_start) = event.workqueue_execute_start_format {
            self.on_workqueue_execute_start(ts, event_tid, &event_comm, workqueue_start);
        } else if event.workqueue_execute_end_format.is_some() {
            self.on_workqueue_execute_end(ts, event_tid);
        } else if let Some(sched_switch) = event.sched_switch_format {
            self.on_sched_switch(ts, cpu, sched_switch)?;
        } else if let Some(sched_wakeup) = event.sched_wakeup_format {
            let target_utid =
                self.on_sched_wakeup(ts, sched_wakeup.pid, Some(sched_wakeup.comm.as_str()));
            self.push_sched_instant(ts, cpu, tid, "sched_wakeup", target_utid);
            self.tables.push_raw_event(RawEventRow {
                ts: Some(ts),
                cpu: Some(cpu),
                tid,
                event_name: "sched_wakeup".to_string(),
                payload_json: Some(
                    json!({
                        "comm": sched_wakeup.comm,
                        "pid": sched_wakeup.pid,
                        "prio": sched_wakeup.prio,
                        "success": sched_wakeup.success,
                        "target_cpu": sched_wakeup.target_cpu
                    })
                    .to_string(),
                ),
            });
        } else if let Some(sched_wakeup_new) = event.sched_wakeup_new_format {
            let target_utid = self.on_sched_wakeup(
                ts,
                sched_wakeup_new.pid,
                Some(sched_wakeup_new.comm.as_str()),
            );
            self.push_sched_instant(ts, cpu, tid, "sched_wakeup", target_utid);
            self.tables.push_raw_event(RawEventRow {
                ts: Some(ts),
                cpu: Some(cpu),
                tid,
                event_name: "sched_wakeup_new".to_string(),
                payload_json: Some(
                    json!({
                        "comm": sched_wakeup_new.comm,
                        "pid": sched_wakeup_new.pid,
                        "prio": sched_wakeup_new.prio,
                        "success": sched_wakeup_new.success,
                        "target_cpu": sched_wakeup_new.target_cpu
                    })
                    .to_string(),
                ),
            });
        } else if let Some(sched_waking) = event.sched_waking_format {
            let target_utid = self.get_or_create_thread(
                ts,
                sanitize_tid(sched_waking.pid),
                Some(sched_waking.comm.as_str()),
            );
            self.push_sched_instant(ts, cpu, tid, "sched_waking", Some(target_utid));
            self.tables.push_raw_event(RawEventRow {
                ts: Some(ts),
                cpu: Some(cpu),
                tid,
                event_name: "sched_waking".to_string(),
                payload_json: Some(
                    json!({
                        "comm": sched_waking.comm,
                        "pid": sched_waking.pid,
                        "prio": sched_waking.prio,
                        "success": sched_waking.success,
                        "target_cpu": sched_waking.target_cpu
                    })
                    .to_string(),
                ),
            });
        } else if let Some(irq_entry) = event.irq_handler_entry_format {
            self.on_irq_entry(ts, cpu, irq_entry);
        } else if let Some(irq_exit) = event.irq_handler_exit_format {
            self.on_irq_exit(ts, cpu, irq_exit.irq);
        } else if let Some(softirq_entry) = event.softirq_entry_format {
            self.on_softirq_entry(ts, cpu, softirq_entry.vec);
        } else if let Some(softirq_exit) = event.softirq_exit_format {
            self.on_softirq_exit(ts, cpu, softirq_exit.vec);
        } else if let Some(softirq_raise) = event.softirq_raise_format {
            self.push_named_raw_event(
                ts,
                cpu,
                tid,
                "softirq_raise",
                json!({
                    "vec": softirq_raise.vec,
                    "name": softirq_name(softirq_raise.vec)
                }),
            );
        } else if let Some(cpu_idle) = event.cpu_idle_format {
            self.on_cpu_idle(ts, cpu, cpu_idle);
        } else if let Some(clock_set_rate) = event.clock_set_rate_format {
            self.on_clock_set_rate(
                ts,
                cpu,
                "clock_set_rate",
                clock_set_rate.name,
                clock_set_rate.state,
            );
        } else if let Some(clk_set_rate) = event.clk_set_rate_format {
            self.on_clock_set_rate(
                ts,
                cpu,
                "clk_set_rate",
                clk_set_rate.name,
                clk_set_rate.rate,
            );
        } else if let Some(clk_set_rate) = event.clk_set_rate_complete_format {
            self.push_named_raw_event(
                ts,
                cpu,
                None,
                "clk_set_rate_complete",
                json!({ "name": clk_set_rate.name, "rate": clk_set_rate.rate }),
            );
        } else if let Some(clk_disable) = event.clk_disable_format {
            self.on_clock_set_rate(ts, cpu, "clk_disable", clk_disable.name, 0);
        } else if let Some(clk_enable) = event.clk_enable_format {
            self.on_clock_set_rate(ts, cpu, "clk_enable", clk_enable.name, 1);
        } else if let Some(cpu_limits) = event.cpu_frequency_limits_format {
            self.on_cpu_frequency_limits(ts, cpu, cpu_limits);
        } else if let Some(dma_fence) = event.dma_fence_destroy_format {
            self.on_dma_fence(ts, cpu, "dma_fence_destroy", dma_fence);
        } else if let Some(dma_fence) = event.dma_fence_emit_format {
            self.on_dma_fence(ts, cpu, "dma_fence_emit", dma_fence);
        } else if let Some(dma_fence) = event.dma_fence_enable_signal_format {
            self.on_dma_fence(ts, cpu, "dma_fence_enable_signal", dma_fence);
        } else if let Some(dma_fence) = event.dma_fence_init_format {
            self.on_dma_fence(ts, cpu, "dma_fence_init", dma_fence);
        } else if let Some(dma_fence) = event.dma_fence_signaled_format {
            self.on_dma_fence(ts, cpu, "dma_fence_signaled", dma_fence);
        } else if let Some(dma_fence) = event.dma_fence_wait_end_format {
            self.on_dma_fence(ts, cpu, "dma_fence_wait_end", dma_fence);
        } else if let Some(dma_fence) = event.dma_fence_wait_start_format {
            self.on_dma_fence(ts, cpu, "dma_fence_wait_start", dma_fence);
        } else {
            self.tables.push_raw_event(RawEventRow {
                ts: Some(ts),
                cpu: Some(cpu),
                tid,
                event_name: "unsupported_ftrace_event".to_string(),
                payload_json: Some(
                    json!({
                        "comm": event.comm,
                        "tgid": event.tgid
                    })
                    .to_string(),
                ),
            });
        }

        Ok(())
    }

    fn push_named_raw_event(
        &mut self,
        ts: i64,
        cpu: u32,
        tid: Option<u32>,
        event_name: &str,
        payload: serde_json::Value,
    ) {
        self.tables.push_raw_event(RawEventRow {
            ts: Some(ts),
            cpu: Some(cpu),
            tid,
            event_name: event_name.to_string(),
            payload_json: Some(payload.to_string()),
        });
    }

    fn push_sched_instant(
        &mut self,
        ts: i64,
        cpu: u32,
        waker_tid: Option<u32>,
        event_name: &str,
        target_utid: Option<u32>,
    ) {
        let wakeup_from = waker_tid.map(|tid| self.get_or_create_thread(ts, tid, None));
        self.tables.push_raw(RawRow {
            id: self.tables.next_raw_id(),
            ts,
            name: event_name.to_string(),
            cpu,
            itid: target_utid,
        });
        self.tables.push_instant(InstantRow {
            ts,
            name: event_name.to_string(),
            ref_id: target_utid,
            wakeup_from,
            ref_type: Some("itid".to_string()),
            value: Some(0.0),
        });
    }

    fn on_sched_switch(&mut self, ts: i64, cpu: u32, msg: SchedSwitchFormat) -> ParseResult<()> {
        let prev_tid = sanitize_tid(msg.prev_pid);
        let next_tid = sanitize_tid(msg.next_pid);
        let prev_utid = self.get_or_create_thread(ts, prev_tid, Some(msg.prev_comm.as_str()));
        let next_utid = self.get_or_create_thread(ts, next_tid, Some(msg.next_comm.as_str()));

        if let Some(open) = self.cpu_running.remove(&cpu) {
            if let Some(row) = self.tables.sched_slice_mut(open.row_id) {
                row.dur = Some(ts.saturating_sub(open.ts));
                row.end_state = Some(state_from_kernel(msg.prev_state));
            }
        }

        let row_id = self.tables.push_sched_slice(SchedSliceRow {
            cpu,
            utid: next_utid,
            ts,
            dur: None,
            priority: Some(msg.next_prio),
            end_state: Some("runnable".to_string()),
        });
        self.cpu_running.insert(cpu, OpenSchedSlice { row_id, ts });

        if prev_tid != 0 {
            self.check_wakeup_event(prev_tid, prev_utid);
            self.transition_thread_state(prev_utid, ts, state_from_kernel(msg.prev_state), None);
        }
        if next_tid != 0 {
            self.check_wakeup_event(next_tid, next_utid);
            self.transition_thread_state(next_utid, ts, "running".to_string(), None);
        }
        Ok(())
    }

    fn on_sched_wakeup(&mut self, ts: i64, pid: i32, name: Option<&str>) -> Option<u32> {
        let tid = sanitize_tid(pid);
        if tid == 0 {
            return None;
        }
        let utid = self.get_or_create_thread(ts, tid, name);
        self.pending_wakeup_by_tid.entry(tid).or_insert(ts);
        Some(utid)
    }

    fn on_irq_entry(&mut self, ts: i64, cpu: u32, event: IrqHandlerEntryFormat) {
        let callid = event.irq;
        let id = self.tables.next_irq_id();
        let row_id = self.tables.push_irq(IrqRow {
            id,
            ts,
            dur: None,
            callid: Some(callid),
            cat: "irq".to_string(),
            name: event.name.clone(),
            depth: Some(0),
            cookie: None,
            parent_id: None,
            argsetid: Some(id),
            flag: Some("1".to_string()),
        });
        self.open_irqs
            .entry(IrqKey {
                cpu,
                cat: "irq",
                callid,
            })
            .or_default()
            .push(row_id);
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            "irq_handler_entry",
            json!({ "irq": event.irq, "name": event.name }),
        );
    }

    fn on_irq_exit(&mut self, ts: i64, cpu: u32, irq: i32) {
        self.close_irq(
            ts,
            IrqKey {
                cpu,
                cat: "irq",
                callid: irq,
            },
        );
        self.push_named_raw_event(ts, cpu, None, "irq_handler_exit", json!({ "irq": irq }));
    }

    fn on_softirq_entry(&mut self, ts: i64, cpu: u32, vec: u32) {
        let callid = vec as i32;
        let id = self.tables.next_irq_id();
        let row_id = self.tables.push_irq(IrqRow {
            id,
            ts,
            dur: None,
            callid: Some(callid),
            cat: "softirq".to_string(),
            name: softirq_name(vec).to_string(),
            depth: Some(0),
            cookie: None,
            parent_id: None,
            argsetid: Some(id),
            flag: Some("1".to_string()),
        });
        self.open_irqs
            .entry(IrqKey {
                cpu,
                cat: "softirq",
                callid,
            })
            .or_default()
            .push(row_id);
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            "softirq_entry",
            json!({ "vec": vec, "name": softirq_name(vec) }),
        );
    }

    fn on_softirq_exit(&mut self, ts: i64, cpu: u32, vec: u32) {
        self.close_irq(
            ts,
            IrqKey {
                cpu,
                cat: "softirq",
                callid: vec as i32,
            },
        );
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            "softirq_exit",
            json!({ "vec": vec, "name": softirq_name(vec) }),
        );
    }

    fn close_irq(&mut self, ts: i64, key: IrqKey) {
        let Some(stack) = self.open_irqs.get_mut(&key) else {
            return;
        };
        let Some(row_id) = stack.pop() else {
            return;
        };
        if let Some(row) = self.tables.irq_mut(row_id) {
            row.dur = Some(ts.saturating_sub(row.ts));
        }
    }

    fn on_cpu_idle(&mut self, ts: i64, cpu: u32, event: CpuIdleFormat) {
        self.tables.push_raw(RawRow {
            id: self.tables.next_raw_id(),
            ts,
            name: "cpu_idle".to_string(),
            cpu,
            itid: Some(0),
        });
        let filter_id = self.measure_filter("cpu_idle", "cpu_measure_filter", Some(event.cpu_id));
        self.push_measure(ts, filter_id, event.state as i64);
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            "cpu_idle",
            json!({ "state": event.state, "cpu_id": event.cpu_id }),
        );
    }

    fn on_clock_set_rate(&mut self, ts: i64, cpu: u32, event_name: &str, name: String, value: u64) {
        let filter_id = self.measure_filter(&name, "measure_filter", None);
        self.push_measure(ts, filter_id, value as i64);
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            event_name,
            json!({ "name": name, "value": value }),
        );
    }

    fn on_cpu_frequency_limits(&mut self, ts: i64, cpu: u32, event: CpuFrequencyLimitsFormat) {
        let max_filter = self.measure_filter(
            "cpu_frequency_limits_max",
            "cpu_measure_filter",
            Some(event.cpu_id),
        );
        self.push_measure(ts, max_filter, event.max_freq as i64);
        let min_filter = self.measure_filter(
            "cpu_frequency_limits_min",
            "cpu_measure_filter",
            Some(event.cpu_id),
        );
        self.push_measure(ts, min_filter, event.min_freq as i64);
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            "cpu_frequency_limits",
            json!({ "min_freq": event.min_freq, "max_freq": event.max_freq, "cpu_id": event.cpu_id }),
        );
    }

    fn on_dma_fence(&mut self, ts: i64, cpu: u32, event_name: &str, event: DmaFenceFormat) {
        self.tables.push_dma_fence(DmaFenceRow {
            id: self.tables.next_dma_fence_id(),
            ts,
            dur: None,
            cat: event_name.to_string(),
            driver: event.driver.clone(),
            timeline: event.timeline.clone(),
            context: event.context,
            seqno: event.seqno,
        });
        self.push_named_raw_event(
            ts,
            cpu,
            None,
            event_name,
            json!({
                "driver": event.driver,
                "timeline": event.timeline,
                "context": event.context,
                "seqno": event.seqno
            }),
        );
    }

    fn measure_filter(&mut self, name: &str, filter_type: &str, cpu: Option<u32>) -> u64 {
        let key = (name.to_string(), filter_type.to_string(), cpu);
        if let Some(id) = self.measure_filters.get(&key) {
            return *id;
        }
        let id = self.next_measure_filter_id;
        self.next_measure_filter_id += 1;
        self.tables.push_measure_filter(MeasureFilterRow {
            id,
            name: name.to_string(),
            source_arg_set_id: None,
            filter_type: filter_type.to_string(),
        });
        if let Some(cpu) = cpu {
            self.tables.push_cpu_measure_filter(CpuMeasureFilterRow {
                id,
                name: name.to_string(),
                cpu,
            });
        }
        self.measure_filters.insert(key, id);
        id
    }

    fn push_measure(&mut self, ts: i64, filter_id: u64, value: i64) {
        if let Some(open_row) = self.open_measures.insert(filter_id, usize::MAX) {
            if open_row != usize::MAX {
                if let Some(row) = self.tables.measure_mut(open_row) {
                    row.dur = Some(ts.saturating_sub(row.ts));
                }
            }
        }
        let row_id = self.tables.push_measure(MeasureRow {
            measure_type: "measure".to_string(),
            ts,
            dur: None,
            value,
            filter_id,
        });
        self.open_measures.insert(filter_id, row_id);
    }

    fn check_wakeup_event(&mut self, tid: u32, utid: u32) {
        let Some(wakeup_ts) = self.pending_wakeup_by_tid.remove(&tid) else {
            return;
        };

        if let Some(row_id) = self.thread_state_open.get(&utid).copied() {
            let Some(row) = self.tables.thread_state_mut(row_id) else {
                return;
            };
            if row.state == "running" {
                return;
            }
            row.dur = Some(wakeup_ts.saturating_sub(row.ts));
        }

        let row_id = self.tables.push_thread_state(ThreadStateRow {
            utid,
            ts: wakeup_ts,
            dur: None,
            state: "runnable".to_string(),
            io_wait: None,
            blocked_function: None,
            waker_utid: None,
        });
        self.thread_state_open.insert(utid, row_id);
    }

    fn transition_thread_state(
        &mut self,
        utid: u32,
        ts: i64,
        state: String,
        waker_utid: Option<u32>,
    ) {
        if let Some(row_id) = self.thread_state_open.remove(&utid) {
            if let Some(row) = self.tables.thread_state_mut(row_id) {
                row.dur = Some(ts.saturating_sub(row.ts));
            }
        }

        let row_id = self.tables.push_thread_state(ThreadStateRow {
            utid,
            ts,
            dur: None,
            state,
            io_wait: None,
            blocked_function: None,
            waker_utid,
        });
        self.thread_state_open.insert(utid, row_id);
    }

    fn get_or_create_thread(&mut self, ts: i64, tid: u32, name: Option<&str>) -> u32 {
        if let Some(info) = self.threads_by_tid.get_mut(&tid) {
            if let Some(name) = name.filter(|s| !s.is_empty()) {
                info.name = Some(name.to_string());
            }
            info.end_ts = Some(ts);
            return info.utid;
        }

        let upid = self.get_or_create_process(ts, tid, name);
        let id = self.next_id;
        self.next_id += 1;
        let name = name
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| (tid == 0).then(|| "idle".to_string()));

        self.threads_by_tid.insert(
            tid,
            ThreadInfo {
                utid: id,
                tid,
                upid,
                name,
                end_ts: Some(ts),
            },
        );
        id
    }

    fn get_or_create_process(&mut self, ts: i64, pid: u32, name: Option<&str>) -> u32 {
        if let Some(info) = self.processes_by_pid.get_mut(&pid) {
            if let Some(name) = name.filter(|s| !s.is_empty()) {
                info.name = Some(name.to_string());
            }
            info.end_ts = Some(ts);
            return info.upid;
        }

        let upid = self.next_id;
        self.next_id += 1;
        let name = name
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| (pid == 0).then(|| "idle".to_string()));
        self.processes_by_pid.insert(
            pid,
            ProcessInfoState {
                upid,
                pid,
                name,
                start_ts: Some(ts),
                end_ts: Some(ts),
            },
        );
        upid
    }

    fn observe_ts(&mut self, ts: i64) {
        self.start_ts = Some(self.start_ts.map_or(ts, |current| current.min(ts)));
        self.end_ts = Some(self.end_ts.map_or(ts, |current| current.max(ts)));
    }

    fn finish_open_intervals(&mut self) {
        // Match TraceStreamer: intervals still open at EOF keep NULL duration.
    }

    fn finish(mut self) -> ParseResult<ParsedTrace> {
        self.process_pending_ftrace_events()?;
        process::finish_live_process(&mut self.tables, &mut self.live_process_state);
        self.finish_open_intervals();

        for info in self.processes_by_pid.values() {
            self.tables.push_process(ProcessRow {
                upid: info.upid,
                pid: info.pid,
                name: info.name.clone(),
                start_ts: info.start_ts,
                end_ts: info.end_ts,
            });
        }

        for info in self.threads_by_tid.values() {
            self.tables.push_thread(ThreadRow {
                utid: info.utid,
                tid: info.tid,
                upid: info.upid,
                name: info.name.clone(),
                is_main: true,
            });
        }

        let trace_id = format!("htrace:{:016x}", self.input_hash);
        let tables = self
            .tables
            .finish(
                trace_id.clone(),
                self.start_ts,
                self.end_ts,
                self.clock_domain.clone(),
            )
            .map_err(|err| {
                TraceEngineError::Engine(format!("failed to build Arrow tables: {err}"))
            })?;

        Ok(ParsedTrace {
            trace_id,
            start_ts: self.start_ts,
            end_ts: self.end_ts,
            clock_domain: self.clock_domain,
            tables,
        })
    }
}

impl HarmonyTraceParser for HtraceParser {
    fn parse_file(&mut self, path: &Path) -> ParseResult<ParsedTrace> {
        let bytes = fs::read(path)?;
        self.parse_bytes(&bytes)
    }

    fn parse_bytes(&mut self, bytes: &[u8]) -> ParseResult<ParsedTrace> {
        self.reset_for_input(bytes);
        if has_profiler_header(bytes) {
            self.parse_framed_file(bytes)?;
        } else {
            self.parse_len_prefixed_segments(bytes)?;
        }
        let parser = std::mem::take(self);
        parser.finish()
    }
}

fn has_profiler_header(bytes: &[u8]) -> bool {
    bytes.len() >= PROFILER_HEADER_SIZE
        && read_u64_le(bytes, 0)
            .map(|magic| magic == PROFILER_HEADER_MAGIC)
            .unwrap_or(false)
}

fn plugin_outer_ts(plugin: &ProfilerPluginData) -> Option<i64> {
    if plugin.tv_sec == 0 && plugin.tv_nsec == 0 {
        return None;
    }
    Some(
        plugin
            .tv_sec
            .saturating_mul(1_000_000_000)
            .saturating_add(plugin.tv_nsec) as i64,
    )
}

fn convert_clock_with_offsets(offsets: Option<&BTreeMap<u64, i128>>, src_ts: u64) -> u64 {
    let Some(offsets) = offsets else {
        return src_ts;
    };
    let Some((_, offset)) = offsets.range(..=src_ts).next_back() else {
        return src_ts;
    };
    let converted = src_ts as i128 + *offset;
    converted.clamp(0, u64::MAX as i128) as u64
}

fn non_empty_str(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

fn binder_flags_desc(flags: u32) -> String {
    let mut desc = String::new();
    if (flags & BINDER_ONEWAY_FLAG) == BINDER_ONEWAY_FLAG {
        desc.push_str(" this is a one-way call: async, no return; ");
    }
    if (flags & BINDER_ROOT_OBJECT_FLAG) == BINDER_ROOT_OBJECT_FLAG {
        desc.push_str(" contents are the components root object; ");
    }
    if (flags & BINDER_STATUS_CODE_FLAG) == BINDER_STATUS_CODE_FLAG {
        desc.push_str(" contents are a 32-bit status code; ");
    }
    if (flags & BINDER_ACCEPT_FDS_FLAG) == BINDER_ACCEPT_FDS_FLAG {
        desc.push_str(" allow replies with file descriptors; ");
    }
    if flags == 0 {
        desc.push_str(" No Flags Set");
    }
    desc
}

fn sample_ts_to_ns(ts: &SampleTimeStamp) -> i64 {
    ts.tv_sec
        .saturating_mul(1_000_000_000)
        .saturating_add(ts.tv_nsec) as i64
}

fn collect_ts_to_ns(ts: &CollectTimeStamp) -> i64 {
    ts.tv_sec
        .saturating_mul(1_000_000_000)
        .saturating_add(ts.tv_nsec) as i64
}

fn sanitize_tid(tid: i32) -> u32 {
    u32::try_from(tid).unwrap_or(0)
}

fn state_from_kernel(state: u64) -> String {
    match state {
        0 => "runnable".to_string(),
        1 => "sleeping".to_string(),
        2 => "uninterruptible".to_string(),
        4 => "stopped".to_string(),
        8 => "traced".to_string(),
        16 | 32 => "exit".to_string(),
        64 => "parked".to_string(),
        128 => "dead".to_string(),
        256 | 2048 | 2049 => "runnable".to_string(),
        value => format!("state_{value}"),
    }
}

fn softirq_name(vec: u32) -> &'static str {
    match vec {
        0 => "HI",
        1 => "TIMER",
        2 => "NET_TX",
        3 => "NET_RX",
        4 => "BLOCK",
        5 => "IRQ_POLL",
        6 => "TASKLET",
        7 => "SCHED",
        8 => "HRTIMER",
        9 => "RCU",
        _ => "UNKNOWN",
    }
}

fn read_u32_le(bytes: &[u8], offset: usize) -> ParseResult<u32> {
    let end = offset + 4;
    let data = bytes
        .get(offset..end)
        .ok_or_else(|| TraceEngineError::Parse(format!("missing u32 at byte {offset}")))?;
    Ok(u32::from_le_bytes(
        data.try_into().expect("slice has length 4"),
    ))
}

fn read_u64_le(bytes: &[u8], offset: usize) -> ParseResult<u64> {
    let end = offset + 8;
    let data = bytes
        .get(offset..end)
        .ok_or_else(|| TraceEngineError::Parse(format!("missing u64 at byte {offset}")))?;
    Ok(u64::from_le_bytes(
        data.try_into().expect("slice has length 8"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::{arkts, memory, process};

    fn len_prefixed(plugin: ProfilerPluginData) -> Vec<u8> {
        let segment = plugin.encode_to_vec();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(segment.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&segment);
        bytes
    }

    fn append_segment(bytes: &mut Vec<u8>, plugin: ProfilerPluginData) {
        let segment = plugin.encode_to_vec();
        bytes.extend_from_slice(&(segment.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&segment);
    }

    #[test]
    fn parses_len_prefixed_sched_switches() {
        let first = FtraceEvent {
            timestamp: 100,
            tgid: 1,
            comm: "prev".to_string(),
            common_fields: Some(FtraceEventCommonFields {
                event_type: 0,
                flags: 0,
                preempt_count: 0,
                pid: 1,
            }),
            sched_switch_format: Some(SchedSwitchFormat {
                prev_comm: "idle".to_string(),
                prev_pid: 0,
                prev_prio: 120,
                prev_state: 0,
                next_comm: "worker".to_string(),
                next_pid: 10,
                next_prio: 110,
            }),
            sched_wakeup_format: None,
            sched_wakeup_new_format: None,
            sched_waking_format: None,
            ..Default::default()
        };
        let second = FtraceEvent {
            timestamp: 150,
            tgid: 1,
            comm: "worker".to_string(),
            common_fields: Some(FtraceEventCommonFields {
                event_type: 0,
                flags: 0,
                preempt_count: 0,
                pid: 10,
            }),
            sched_switch_format: Some(SchedSwitchFormat {
                prev_comm: "worker".to_string(),
                prev_pid: 10,
                prev_prio: 110,
                prev_state: 1,
                next_comm: "idle".to_string(),
                next_pid: 0,
                next_prio: 120,
            }),
            sched_wakeup_format: None,
            sched_wakeup_new_format: None,
            sched_waking_format: None,
            ..Default::default()
        };

        let trace = TracePluginResult {
            ftrace_cpu_stats: vec![FtraceCpuStatsMsg {
                trace_clock: "boot".to_string(),
            }],
            ftrace_cpu_detail: vec![FtraceCpuDetailMsg {
                cpu: 0,
                event: vec![first, second],
                overwrite: 0,
            }],
            symbols_detail: vec![],
            clocks_detail: vec![],
        };

        let plugin = ProfilerPluginData {
            name: "ftrace-plugin".to_string(),
            status: 0,
            data: trace.encode_to_vec(),
            clock_id: 7,
            tv_sec: 0,
            tv_nsec: 0,
            version: "1.01".to_string(),
            sample_interval: 0,
        };

        let bytes = len_prefixed(plugin);

        let parsed = HtraceParser::default().parse_bytes(&bytes).unwrap();
        assert_eq!(parsed.clock_domain, "boottime");
        assert_eq!(parsed.tables.sched_slice.num_rows(), 2);
        assert_eq!(parsed.tables.thread.num_rows(), 2);
    }

    #[test]
    fn sorts_ftrace_events_across_cpu_details_before_filtering() {
        let switch = FtraceEvent {
            timestamp: 200,
            tgid: 1,
            comm: "worker".to_string(),
            common_fields: Some(FtraceEventCommonFields {
                event_type: 0,
                flags: 0,
                preempt_count: 0,
                pid: 0,
            }),
            sched_switch_format: Some(SchedSwitchFormat {
                prev_comm: "idle".to_string(),
                prev_pid: 0,
                prev_prio: 120,
                prev_state: 0,
                next_comm: "worker".to_string(),
                next_pid: 10,
                next_prio: 110,
            }),
            sched_wakeup_format: None,
            sched_wakeup_new_format: None,
            sched_waking_format: None,
            ..Default::default()
        };
        let wakeup = FtraceEvent {
            timestamp: 100,
            tgid: 1,
            comm: "waker".to_string(),
            common_fields: Some(FtraceEventCommonFields {
                event_type: 0,
                flags: 0,
                preempt_count: 0,
                pid: 20,
            }),
            sched_switch_format: None,
            sched_wakeup_format: Some(SchedWakeupFormat {
                comm: "worker".to_string(),
                pid: 10,
                prio: 110,
                success: 1,
                target_cpu: 0,
            }),
            sched_wakeup_new_format: None,
            sched_waking_format: None,
            ..Default::default()
        };

        let trace = TracePluginResult {
            ftrace_cpu_stats: vec![],
            ftrace_cpu_detail: vec![
                FtraceCpuDetailMsg {
                    cpu: 0,
                    event: vec![switch],
                    overwrite: 0,
                },
                FtraceCpuDetailMsg {
                    cpu: 1,
                    event: vec![wakeup],
                    overwrite: 0,
                },
            ],
            symbols_detail: vec![],
            clocks_detail: vec![],
        };

        let plugin = ProfilerPluginData {
            name: "ftrace-plugin".to_string(),
            status: 0,
            data: trace.encode_to_vec(),
            clock_id: 7,
            tv_sec: 0,
            tv_nsec: 0,
            version: "1.01".to_string(),
            sample_interval: 0,
        };

        let bytes = len_prefixed(plugin);

        let parsed = HtraceParser::default().parse_bytes(&bytes).unwrap();
        assert_eq!(parsed.tables.thread_state.num_rows(), 2);
    }

    #[test]
    fn parses_memory_plugin_process_and_system_measures() {
        let memory_data = memory::MemoryData {
            processesinfo: vec![memory::ProcessMemoryInfo {
                pid: 42,
                name: "com.demo".to_string(),
                vm_size_kb: 1000,
                vm_rss_kb: 200,
                rss_anon_kb: 120,
                rss_file_kb: 60,
                rss_shmem_kb: 20,
                vm_swap_kb: 5,
                vm_locked_kb: 1,
                vm_hwm_kb: 220,
                oom_score_adj: 100,
                ..Default::default()
            }],
            meminfo: vec![memory::SysMeminfo {
                key: 1,
                value: 8192,
            }],
            ..Default::default()
        };
        let plugin = ProfilerPluginData {
            name: "memory-plugin".to_string(),
            status: 0,
            data: memory_data.encode_to_vec(),
            clock_id: 0,
            tv_sec: 1,
            tv_nsec: 0,
            version: "1.01".to_string(),
            sample_interval: 0,
        };

        let parsed = HtraceParser::default()
            .parse_bytes(&len_prefixed(plugin))
            .unwrap();
        assert_eq!(parsed.tables.process_measure.num_rows(), 9);
        assert_eq!(parsed.tables.process_measure_filter.num_rows(), 9);
        assert_eq!(parsed.tables.sys_mem_measure.num_rows(), 4);
        assert_eq!(parsed.tables.sys_event_filter.num_rows(), 4);
        assert_eq!(parsed.tables.process.num_rows(), 1);
    }

    #[test]
    fn parses_process_plugin_live_process_samples() {
        let mut bytes = Vec::new();
        for (ts, cpu_time, pss) in [(10, 100, 2048), (20, 140, 3072)] {
            let data = process::ProcessData {
                processesinfo: vec![process::ProcessInfo {
                    pid: 42,
                    name: "com.demo".to_string(),
                    ppid: 1,
                    uid: 200100,
                    cpuinfo: Some(process::CpuInfo {
                        cpu_usage: 12.5,
                        thread_sum: 8,
                        cpu_time_ms: cpu_time,
                    }),
                    pssinfo: Some(process::PssInfo { pss_info: pss }),
                    diskinfo: Some(process::DiskioInfo {
                        rbytes: 11,
                        wbytes: 22,
                        ..Default::default()
                    }),
                }],
            };
            append_segment(
                &mut bytes,
                ProfilerPluginData {
                    name: "process-plugin".to_string(),
                    status: 0,
                    data: data.encode_to_vec(),
                    clock_id: 0,
                    tv_sec: ts,
                    tv_nsec: 0,
                    version: "1.01".to_string(),
                    sample_interval: 0,
                },
            );
        }

        let parsed = HtraceParser::default().parse_bytes(&bytes).unwrap();
        assert_eq!(parsed.tables.live_process.num_rows(), 1);
    }

    #[test]
    fn parses_arkts_js_heap_snapshot() {
        let heap_json = r#"{
            "snapshot":{
                "meta":{
                    "node_fields":["type","name","id","self_size","edge_count","trace_node_id","detachedness"],
                    "node_types":[["hidden","object"],"string","number","number","number","number","number"],
                    "edge_fields":["type","name_or_index","to_node"],
                    "edge_types":[["context","element"],"string","node"],
                    "trace_function_info_fields":["function_id","name","script_name","script_id","line","column"],
                    "trace_node_fields":["id","function_info_index","count","size","children"],
                    "sample_fields":["timestamp_us","last_assigned_id"],
                    "location_fields":["object_index","script_id","line","column"]
                },
                "node_count":2,
                "edge_count":1,
                "trace_function_count":1
            },
            "nodes":[1,0,10,64,1,0,0,1,1,20,32,0,0,0],
            "edges":[1,2,7],
            "locations":[],
            "samples":[5,20],
            "strings":["","Object"],
            "trace_function_infos":[1,0,1,7,10,2],
            "trace_tree":[1,0,1,64,[]]
        }"#;

        let mut bytes = Vec::new();
        append_segment(
            &mut bytes,
            ProfilerPluginData {
                name: "arkts-plugin_config".to_string(),
                status: 0,
                data: arkts::ArkTSConfig {
                    pid: 42,
                    heap_type: arkts::HeapType::Snapshot as i32,
                    interval: 1,
                    capture_numeric_value: false,
                    track_allocations: false,
                    enable_cpu_profiler: false,
                    cpu_profiler_interval: 0,
                }
                .encode_to_vec(),
                clock_id: 0,
                tv_sec: 0,
                tv_nsec: 0,
                version: "1.01".to_string(),
                sample_interval: 0,
            },
        );
        append_segment(
            &mut bytes,
            ProfilerPluginData {
                name: "arkts-plugin".to_string(),
                status: 0,
                data: arkts::ArkTSResult {
                    result: format!(
                        r#"{{"params":{{"chunk":{}}}}}"#,
                        serde_json::to_string(heap_json).unwrap()
                    )
                    .into_bytes(),
                }
                .encode_to_vec(),
                clock_id: 0,
                tv_sec: 1,
                tv_nsec: 0,
                version: "1.01".to_string(),
                sample_interval: 0,
            },
        );
        append_segment(
            &mut bytes,
            ProfilerPluginData {
                name: "arkts-plugin".to_string(),
                status: 0,
                data: arkts::ArkTSResult {
                    result: b"{\"id\":1,\"result\":{}}".to_vec(),
                }
                .encode_to_vec(),
                clock_id: 0,
                tv_sec: 2,
                tv_nsec: 0,
                version: "1.01".to_string(),
                sample_interval: 0,
            },
        );

        let parsed = HtraceParser::default().parse_bytes(&bytes).unwrap();
        assert_eq!(parsed.tables.js_heap_files.num_rows(), 1);
        assert_eq!(parsed.tables.js_heap_nodes.num_rows(), 2);
        assert_eq!(parsed.tables.js_heap_edges.num_rows(), 1);
        assert_eq!(parsed.tables.js_heap_string.num_rows(), 2);
        assert_eq!(parsed.tables.js_heap_trace_node.num_rows(), 1);
    }

    #[test]
    fn parses_arkts_js_cpu_profiler() {
        let profile_json = r#"{
            "nodes":[
                {
                    "id":1,
                    "callFrame":{
                        "functionName":"(root)",
                        "scriptId":"0",
                        "url":"",
                        "lineNumber":0,
                        "columnNumber":0
                    },
                    "hitCount":1,
                    "children":[2]
                },
                {
                    "id":2,
                    "callFrame":{
                        "functionName":"work",
                        "scriptId":"1",
                        "url":"entry.js",
                        "lineNumber":10,
                        "columnNumber":2
                    },
                    "hitCount":3,
                    "children":[]
                }
            ],
            "samples":[1,1,2],
            "timeDeltas":[0,5,7],
            "startTime":100
        }"#;

        let mut bytes = Vec::new();
        append_segment(
            &mut bytes,
            ProfilerPluginData {
                name: "arkts-plugin_config".to_string(),
                status: 0,
                data: arkts::ArkTSConfig {
                    pid: 42,
                    heap_type: arkts::HeapType::Snapshot as i32,
                    interval: 1,
                    capture_numeric_value: false,
                    track_allocations: false,
                    enable_cpu_profiler: true,
                    cpu_profiler_interval: 1000,
                }
                .encode_to_vec(),
                clock_id: 0,
                tv_sec: 0,
                tv_nsec: 0,
                version: "1.01".to_string(),
                sample_interval: 0,
            },
        );
        append_segment(
            &mut bytes,
            ProfilerPluginData {
                name: "arkts-plugin".to_string(),
                status: 0,
                data: arkts::ArkTSResult {
                    result: format!(r#"{{"id":3,"result":{{"profile":{}}}}}"#, profile_json)
                        .into_bytes(),
                }
                .encode_to_vec(),
                clock_id: 0,
                tv_sec: 1,
                tv_nsec: 0,
                version: "1.01".to_string(),
                sample_interval: 0,
            },
        );

        let parsed = HtraceParser::default().parse_bytes(&bytes).unwrap();
        assert_eq!(parsed.tables.js_config.num_rows(), 1);
        assert_eq!(parsed.tables.js_cpu_profiler_node.num_rows(), 2);
        assert_eq!(parsed.tables.js_cpu_profiler_sample.num_rows(), 2);
    }
}
