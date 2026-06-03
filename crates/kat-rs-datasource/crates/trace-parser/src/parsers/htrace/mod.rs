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

mod binder;
mod clock;
mod framing;
mod ftrace;
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
        let non_empty_tables = tables
            .batches()
            .into_iter()
            .filter_map(|(name, batch)| (batch.num_rows() > 0).then_some((name, batch.num_rows())))
            .collect::<Vec<_>>();
        log::debug!(
            target: "trace_parser::htrace",
            "parsed htrace trace_id={} start_ts={:?} end_ts={:?} non_empty_tables={:?}",
            trace_id,
            self.start_ts,
            self.end_ts,
            non_empty_tables
        );

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
        let framed = has_profiler_header(bytes);
        log::debug!(
            target: "trace_parser::htrace",
            "parse htrace input bytes={} framed={}",
            bytes.len(),
            framed
        );
        if framed {
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
