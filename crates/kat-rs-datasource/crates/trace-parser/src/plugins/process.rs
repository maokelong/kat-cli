use crate::{TraceEngineError, TraceResult};
use prost::Message;
use trace_model::{LiveProcessRow, TraceTableBuilder};

#[derive(Clone, PartialEq, Message)]
pub struct DiskioInfo {
    #[prost(uint64, tag = "1")]
    pub rchar: u64,
    #[prost(uint64, tag = "2")]
    pub wchar: u64,
    #[prost(uint64, tag = "3")]
    pub syscr: u64,
    #[prost(uint64, tag = "4")]
    pub syscw: u64,
    #[prost(uint64, tag = "5")]
    pub rbytes: u64,
    #[prost(uint64, tag = "6")]
    pub wbytes: u64,
    #[prost(uint64, tag = "7")]
    pub cancelled_wbytes: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct PssInfo {
    #[prost(int32, tag = "1")]
    pub pss_info: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct CpuInfo {
    #[prost(double, tag = "1")]
    pub cpu_usage: f64,
    #[prost(int32, tag = "2")]
    pub thread_sum: i32,
    #[prost(uint64, tag = "3")]
    pub cpu_time_ms: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct ProcessInfo {
    #[prost(int32, tag = "1")]
    pub pid: i32,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(int32, tag = "3")]
    pub ppid: i32,
    #[prost(int32, tag = "4")]
    pub uid: i32,
    #[prost(message, optional, tag = "5")]
    pub cpuinfo: Option<CpuInfo>,
    #[prost(message, optional, tag = "6")]
    pub pssinfo: Option<PssInfo>,
    #[prost(message, optional, tag = "7")]
    pub diskinfo: Option<DiskioInfo>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ProcessData {
    #[prost(message, repeated, tag = "1")]
    pub processesinfo: Vec<ProcessInfo>,
}

#[derive(Default)]
pub struct LiveProcessState {
    pending: Vec<LiveProcessSample>,
}

#[derive(Debug, Clone)]
struct LiveProcessSample {
    ts: i64,
    process: ProcessInfo,
}

pub fn parse_process_plugin(
    data: &[u8],
    ts: Option<i64>,
    state: &mut LiveProcessState,
) -> TraceResult<()> {
    let Some(ts) = ts else {
        log::debug!(
            target: "trace_parser::plugins::process",
            "skip process plugin without timestamp data_len={}",
            data.len()
        );
        return Ok(());
    };
    let process_data = ProcessData::decode(data)
        .map_err(|err| TraceEngineError::Parse(format!("failed to decode ProcessData: {err}")))?;
    log::debug!(
        target: "trace_parser::plugins::process",
        "decoded process plugin ts={} processes={}",
        ts,
        process_data.processesinfo.len()
    );
    for process in process_data.processesinfo {
        state.pending.push(LiveProcessSample { ts, process });
    }
    Ok(())
}

pub fn finish_live_process(tables: &mut TraceTableBuilder, state: &mut LiveProcessState) {
    state.pending.sort_by_key(|sample| sample.ts);
    let mut last_ts = None;
    let mut emitted_rows = 0usize;
    for sample in state.pending.drain(..) {
        let Some(previous_ts) = last_ts.replace(sample.ts) else {
            continue;
        };
        if sample.process.pid == 0 {
            continue;
        }
        let cpu = sample.process.cpuinfo.unwrap_or_default();
        let pss = sample.process.pssinfo.unwrap_or_default();
        let disk = sample.process.diskinfo.unwrap_or_default();
        tables.push_live_process(LiveProcessRow {
            ts: sample.ts,
            dur: sample.ts.saturating_sub(previous_ts),
            cpu_time: cpu.cpu_time_ms,
            process_id: sample.process.pid,
            process_name: sample.process.name,
            parent_process_id: sample.process.ppid,
            uid: sample.process.uid,
            user_name: sample.process.uid.to_string(),
            cpu_usage: cpu.cpu_usage,
            pss_info: pss.pss_info,
            thread_num: cpu.thread_sum,
            disk_writes: disk.wbytes as i64,
            disk_reads: disk.rbytes as i64,
        });
        emitted_rows += 1;
    }
    log::debug!(
        target: "trace_parser::plugins::process",
        "emitted live_process rows={}",
        emitted_rows
    );
}
