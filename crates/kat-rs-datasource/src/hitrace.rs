//! Parses hitrace files into Arrow batches backed by profiler plugin segments.

use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result, bail};
use arrow_array::RecordBatch;
use log::debug;
use prost::Message;
use serde::{Deserialize, Serialize};
use serde_arrow::schema::{SchemaLike, TracingOptions};

use crate::{
    mmap::with_mapped_file,
    proto::{
        ProfilerPluginData, SchedBlockedReasonFormat, SchedKthreadStopFormat,
        SchedKthreadStopRetFormat, SchedMigrateTaskFormat, SchedMoveNumaFormat,
        SchedPiSetprioFormat, SchedProcessExecFormat, SchedProcessExitFormat,
        SchedProcessForkFormat, SchedProcessFreeFormat, SchedProcessWaitFormat,
        SchedStatBlockedFormat, SchedStatIowaitFormat, SchedStatRuntimeFormat,
        SchedStatSleepFormat, SchedStatWaitFormat, SchedStickNumaFormat, SchedSwapNumaFormat,
        SchedSwitchFormat, SchedWaitTaskFormat, SchedWakeIdleWithoutIpiFormat, SchedWakeupFormat,
        SchedWakeupNewFormat, SchedWakingFormat, TracePluginResult,
        kat::hitrace::{FtraceEvent, ftrace_event::Event},
    },
};

pub(crate) const HITRACE_TABLE: &str = "profiler_plugin_data";
pub(crate) const THREAD_STATE_TABLE: &str = "thread_state";
pub(crate) const INSTANT_TABLE: &str = "instant";

const FTRACE_PLUGIN_NAME: &str = "ftrace-plugin";
const PROFILER_HEADER_SIZE: usize = 1024;
const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
const HIPROFILER_PROTOBUF_BIN: u32 = 0;
const SEGMENT_LENGTH_SIZE: usize = 4;

pub(crate) struct HitraceTable {
    pub(crate) name: &'static str,
    pub(crate) batches: Vec<RecordBatch>,
}

pub(crate) struct HitraceTables {
    pub(crate) profiler_plugin_data: Vec<RecordBatch>,
    pub(crate) tables: Vec<HitraceTable>,
}

pub(crate) fn load_hitrace_tables(path: &Path) -> Result<HitraceTables> {
    debug!("building hitrace datasource from {}", path.display());

    let tables = with_mapped_file(path, parse_hitrace_bytes)?;

    debug!(
        "built {} profiler batches and {} derived/event tables",
        tables.profiler_plugin_data.len(),
        tables.tables.len()
    );
    Ok(tables)
}

fn parse_hitrace_bytes(bytes: &[u8]) -> Result<HitraceTables> {
    if !has_profiler_header(bytes) {
        bail!("invalid hitrace file: missing OHOSPROF header");
    }

    parse_hitrace_sections(bytes)
}

fn has_profiler_header(bytes: &[u8]) -> bool {
    bytes.len() >= PROFILER_HEADER_SIZE
        && read_u64_le(bytes, 0)
            .map(|magic| magic == PROFILER_HEADER_MAGIC)
            .unwrap_or(false)
}

fn parse_hitrace_sections(bytes: &[u8]) -> Result<HitraceTables> {
    let mut offset = 0usize;
    let mut profiler_batches = Vec::new();
    let mut sched_rows = SchedRows::default();

    while offset < bytes.len() {
        let section = read_profiler_section(bytes, offset)?;
        offset = section.end;

        if section.data_type != HIPROFILER_PROTOBUF_BIN {
            debug!(
                "skip unsupported profiler section data_type={} section_len={}",
                section.data_type, section.len
            );
            continue;
        }

        let messages = decode_len_prefixed_messages::<ProfilerPluginData>(section.body(bytes))
            .with_context(|| {
                format!("failed to parse profiler section at byte {}", section.start)
            })?;
        decode_sched_rows(&messages, section.start, &mut sched_rows)?;
        let batch = record_batch_from(messages).with_context(|| {
            format!(
                "failed to convert profiler section at byte {} to Arrow",
                section.start
            )
        })?;
        profiler_batches.push(batch);
    }

    Ok(HitraceTables {
        profiler_plugin_data: profiler_batches,
        tables: sched_rows.into_tables()?,
    })
}

fn record_batch_from<T>(rows: Vec<T>) -> Result<RecordBatch>
where
    T: Serialize,
    for<'de> T: Deserialize<'de>,
{
    let fields = Vec::<arrow_schema::FieldRef>::from_type::<T>(TracingOptions::default())?;
    Ok(serde_arrow::to_record_batch(&fields, &rows)?)
}

fn decode_sched_rows(
    messages: &[ProfilerPluginData],
    section_start: usize,
    sched_rows: &mut SchedRows,
) -> Result<()> {
    for message in messages
        .iter()
        .filter(|message| message.name == FTRACE_PLUGIN_NAME)
    {
        let result = TracePluginResult::decode(message.data.as_slice()).with_context(|| {
            format!("failed to decode ftrace payload in profiler section at byte {section_start}")
        })?;
        for detail in result.ftrace_cpu_detail {
            for event in detail.event {
                sched_rows.push_event(detail.cpu, event);
            }
        }
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct SchedEventMeta {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
}

impl SchedEventMeta {
    fn from_event(cpu: u32, event: &FtraceEvent) -> Self {
        Self {
            event_timestamp: event.timestamp,
            event_cpu: cpu,
            event_tgid: event.tgid,
            event_comm: event.comm.clone(),
        }
    }
}

#[derive(Default)]
struct SchedRows {
    sched_blocked_reason: Vec<SchedBlockedReasonRow>,
    sched_kthread_stop: Vec<SchedKthreadStopRow>,
    sched_kthread_stop_ret: Vec<SchedKthreadStopRetRow>,
    sched_migrate_task: Vec<SchedMigrateTaskRow>,
    sched_move_numa: Vec<SchedMoveNumaRow>,
    sched_pi_setprio: Vec<SchedPiSetprioRow>,
    sched_process_exec: Vec<SchedProcessExecRow>,
    sched_process_exit: Vec<SchedProcessExitRow>,
    sched_process_fork: Vec<SchedProcessForkRow>,
    sched_process_free: Vec<SchedProcessFreeRow>,
    sched_process_wait: Vec<SchedProcessWaitRow>,
    sched_stat_blocked: Vec<SchedStatDelayRow>,
    sched_stat_iowait: Vec<SchedStatDelayRow>,
    sched_stat_runtime: Vec<SchedStatRuntimeRow>,
    sched_stat_sleep: Vec<SchedStatDelayRow>,
    sched_stat_wait: Vec<SchedStatDelayRow>,
    sched_stick_numa: Vec<SchedStickNumaRow>,
    sched_swap_numa: Vec<SchedSwapNumaRow>,
    sched_switch: Vec<SchedSwitchRow>,
    sched_wait_task: Vec<SchedWaitTaskRow>,
    sched_wake_idle_without_ipi: Vec<SchedWakeIdleWithoutIpiRow>,
    sched_wakeup: Vec<SchedWakeupRow>,
    sched_wakeup_new: Vec<SchedWakeupRow>,
    sched_waking: Vec<SchedWakeupRow>,
    thread_state: ThreadStateBuilder,
    instant: Vec<InstantRow>,
}

impl SchedRows {
    fn push_event(&mut self, cpu: u32, event: FtraceEvent) {
        let meta = SchedEventMeta::from_event(cpu, &event);
        let Some(event) = event.event else {
            return;
        };

        match event {
            Event::SchedKthreadStopFormat(message) => self
                .sched_kthread_stop
                .push(SchedKthreadStopRow::new(&meta, message)),
            Event::SchedKthreadStopRetFormat(message) => self
                .sched_kthread_stop_ret
                .push(SchedKthreadStopRetRow::new(&meta, message)),
            Event::SchedMigrateTaskFormat(message) => self
                .sched_migrate_task
                .push(SchedMigrateTaskRow::new(&meta, message)),
            Event::SchedMoveNumaFormat(message) => self
                .sched_move_numa
                .push(SchedMoveNumaRow::new(&meta, message)),
            Event::SchedPiSetprioFormat(message) => self
                .sched_pi_setprio
                .push(SchedPiSetprioRow::new(&meta, message)),
            Event::SchedProcessExecFormat(message) => self
                .sched_process_exec
                .push(SchedProcessExecRow::new(&meta, message)),
            Event::SchedProcessExitFormat(message) => self
                .sched_process_exit
                .push(SchedProcessExitRow::new(&meta, message)),
            Event::SchedProcessForkFormat(message) => self
                .sched_process_fork
                .push(SchedProcessForkRow::new(&meta, message)),
            Event::SchedProcessFreeFormat(message) => self
                .sched_process_free
                .push(SchedProcessFreeRow::new(&meta, message)),
            Event::SchedProcessWaitFormat(message) => self
                .sched_process_wait
                .push(SchedProcessWaitRow::new(&meta, message)),
            Event::SchedStatBlockedFormat(message) => self
                .sched_stat_blocked
                .push(SchedStatDelayRow::from_blocked(&meta, message)),
            Event::SchedStatIowaitFormat(message) => self
                .sched_stat_iowait
                .push(SchedStatDelayRow::from_iowait(&meta, message)),
            Event::SchedStatRuntimeFormat(message) => self
                .sched_stat_runtime
                .push(SchedStatRuntimeRow::new(&meta, message)),
            Event::SchedStatSleepFormat(message) => self
                .sched_stat_sleep
                .push(SchedStatDelayRow::from_sleep(&meta, message)),
            Event::SchedStatWaitFormat(message) => self
                .sched_stat_wait
                .push(SchedStatDelayRow::from_wait(&meta, message)),
            Event::SchedStickNumaFormat(message) => self
                .sched_stick_numa
                .push(SchedStickNumaRow::new(&meta, message)),
            Event::SchedSwapNumaFormat(message) => self
                .sched_swap_numa
                .push(SchedSwapNumaRow::new(&meta, message)),
            Event::SchedSwitchFormat(message) => {
                let row = SchedSwitchRow::new(&meta, message);
                self.thread_state.push_switch(&row);
                self.sched_switch.push(row);
            }
            Event::SchedWaitTaskFormat(message) => self
                .sched_wait_task
                .push(SchedWaitTaskRow::new(&meta, message)),
            Event::SchedWakeIdleWithoutIpiFormat(message) => self
                .sched_wake_idle_without_ipi
                .push(SchedWakeIdleWithoutIpiRow::new(&meta, message)),
            Event::SchedWakeupFormat(message) => {
                let row = SchedWakeupRow::from_wakeup(&meta, message);
                self.instant
                    .push(InstantRow::from_wakeup(&row, "sched_wakeup"));
                self.sched_wakeup.push(row);
            }
            Event::SchedWakeupNewFormat(message) => {
                let row = SchedWakeupRow::from_wakeup_new(&meta, message);
                self.instant
                    .push(InstantRow::from_wakeup(&row, "sched_wakeup_new"));
                self.sched_wakeup_new.push(row);
            }
            Event::SchedWakingFormat(message) => {
                let row = SchedWakeupRow::from_waking(&meta, message);
                self.instant
                    .push(InstantRow::from_wakeup(&row, "sched_waking"));
                self.sched_waking.push(row);
            }
            Event::SchedBlockedReasonFormat(message) => self
                .sched_blocked_reason
                .push(SchedBlockedReasonRow::new(&meta, message)),
        }
    }

    fn into_tables(self) -> Result<Vec<HitraceTable>> {
        Ok(vec![
            table_from_rows("sched_blocked_reason", self.sched_blocked_reason)?,
            table_from_rows("sched_kthread_stop", self.sched_kthread_stop)?,
            table_from_rows("sched_kthread_stop_ret", self.sched_kthread_stop_ret)?,
            table_from_rows("sched_migrate_task", self.sched_migrate_task)?,
            table_from_rows("sched_move_numa", self.sched_move_numa)?,
            table_from_rows("sched_pi_setprio", self.sched_pi_setprio)?,
            table_from_rows("sched_process_exec", self.sched_process_exec)?,
            table_from_rows("sched_process_exit", self.sched_process_exit)?,
            table_from_rows("sched_process_fork", self.sched_process_fork)?,
            table_from_rows("sched_process_free", self.sched_process_free)?,
            table_from_rows("sched_process_wait", self.sched_process_wait)?,
            table_from_rows("sched_stat_blocked", self.sched_stat_blocked)?,
            table_from_rows("sched_stat_iowait", self.sched_stat_iowait)?,
            table_from_rows("sched_stat_runtime", self.sched_stat_runtime)?,
            table_from_rows("sched_stat_sleep", self.sched_stat_sleep)?,
            table_from_rows("sched_stat_wait", self.sched_stat_wait)?,
            table_from_rows("sched_stick_numa", self.sched_stick_numa)?,
            table_from_rows("sched_swap_numa", self.sched_swap_numa)?,
            table_from_rows("sched_switch", self.sched_switch)?,
            table_from_rows("sched_wait_task", self.sched_wait_task)?,
            table_from_rows(
                "sched_wake_idle_without_ipi",
                self.sched_wake_idle_without_ipi,
            )?,
            table_from_rows("sched_wakeup", self.sched_wakeup)?,
            table_from_rows("sched_wakeup_new", self.sched_wakeup_new)?,
            table_from_rows("sched_waking", self.sched_waking)?,
            table_from_rows(THREAD_STATE_TABLE, self.thread_state.into_rows())?,
            table_from_rows(INSTANT_TABLE, self.instant)?,
        ])
    }
}

fn table_from_rows<T>(name: &'static str, rows: Vec<T>) -> Result<HitraceTable>
where
    T: Serialize,
    for<'de> T: Deserialize<'de>,
{
    Ok(HitraceTable {
        name,
        batches: vec![
            record_batch_from(rows)
                .with_context(|| format!("failed to convert {name} table to Arrow"))?,
        ],
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedBlockedReasonRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    pid: i32,
    caller: u64,
    io_wait: u32,
}

impl SchedBlockedReasonRow {
    fn new(meta: &SchedEventMeta, message: SchedBlockedReasonFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            pid: message.pid,
            caller: message.caller,
            io_wait: message.io_wait,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedKthreadStopRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    comm: String,
    pid: i32,
}

impl SchedKthreadStopRow {
    fn new(meta: &SchedEventMeta, message: SchedKthreadStopFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            comm: message.comm,
            pid: message.pid,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedKthreadStopRetRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    ret: i32,
}

impl SchedKthreadStopRetRow {
    fn new(meta: &SchedEventMeta, message: SchedKthreadStopRetFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            ret: message.ret,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedMigrateTaskRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    comm: String,
    pid: i32,
    prio: i32,
    orig_cpu: i32,
    dest_cpu: i32,
}

impl SchedMigrateTaskRow {
    fn new(meta: &SchedEventMeta, message: SchedMigrateTaskFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            comm: message.comm,
            pid: message.pid,
            prio: message.prio,
            orig_cpu: message.orig_cpu,
            dest_cpu: message.dest_cpu,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedMoveNumaRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    pid: i32,
    tgid: i32,
    ngid: i32,
    src_cpu: i32,
    src_nid: i32,
    dst_cpu: i32,
    dst_nid: i32,
}

impl SchedMoveNumaRow {
    fn new(meta: &SchedEventMeta, message: SchedMoveNumaFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            pid: message.pid,
            tgid: message.tgid,
            ngid: message.ngid,
            src_cpu: message.src_cpu,
            src_nid: message.src_nid,
            dst_cpu: message.dst_cpu,
            dst_nid: message.dst_nid,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedPiSetprioRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    comm: String,
    pid: i32,
    oldprio: i32,
    newprio: i32,
}

impl SchedPiSetprioRow {
    fn new(meta: &SchedEventMeta, message: SchedPiSetprioFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            comm: message.comm,
            pid: message.pid,
            oldprio: message.oldprio,
            newprio: message.newprio,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedProcessExecRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    filename: String,
    pid: i32,
    old_pid: i32,
}

impl SchedProcessExecRow {
    fn new(meta: &SchedEventMeta, message: SchedProcessExecFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            filename: message.filename,
            pid: message.pid,
            old_pid: message.old_pid,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedProcessExitRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    comm: String,
    pid: i32,
    prio: i32,
}

impl SchedProcessExitRow {
    fn new(meta: &SchedEventMeta, message: SchedProcessExitFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            comm: message.comm,
            pid: message.pid,
            prio: message.prio,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedProcessForkRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    parent_comm: String,
    parent_pid: i32,
    child_comm: String,
    child_pid: i32,
}

impl SchedProcessForkRow {
    fn new(meta: &SchedEventMeta, message: SchedProcessForkFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            parent_comm: message.parent_comm,
            parent_pid: message.parent_pid,
            child_comm: message.child_comm,
            child_pid: message.child_pid,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedProcessFreeRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    comm: String,
    pid: i32,
    prio: i32,
}

impl SchedProcessFreeRow {
    fn new(meta: &SchedEventMeta, message: SchedProcessFreeFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            comm: message.comm,
            pid: message.pid,
            prio: message.prio,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedProcessWaitRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    comm: String,
    pid: i32,
    prio: i32,
}

impl SchedProcessWaitRow {
    fn new(meta: &SchedEventMeta, message: SchedProcessWaitFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            comm: message.comm,
            pid: message.pid,
            prio: message.prio,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedStatDelayRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    comm: String,
    pid: i32,
    delay: u64,
}

impl SchedStatDelayRow {
    fn from_blocked(meta: &SchedEventMeta, message: SchedStatBlockedFormat) -> Self {
        Self::new(meta, message.comm, message.pid, message.delay)
    }

    fn from_iowait(meta: &SchedEventMeta, message: SchedStatIowaitFormat) -> Self {
        Self::new(meta, message.comm, message.pid, message.delay)
    }

    fn from_sleep(meta: &SchedEventMeta, message: SchedStatSleepFormat) -> Self {
        Self::new(meta, message.comm, message.pid, message.delay)
    }

    fn from_wait(meta: &SchedEventMeta, message: SchedStatWaitFormat) -> Self {
        Self::new(meta, message.comm, message.pid, message.delay)
    }

    fn new(meta: &SchedEventMeta, comm: String, pid: i32, delay: u64) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            comm,
            pid,
            delay,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedStatRuntimeRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    comm: String,
    pid: i32,
    runtime: u64,
    vruntime: u64,
}

impl SchedStatRuntimeRow {
    fn new(meta: &SchedEventMeta, message: SchedStatRuntimeFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            comm: message.comm,
            pid: message.pid,
            runtime: message.runtime,
            vruntime: message.vruntime,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedStickNumaRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    pid: i32,
    tgid: i32,
    ngid: i32,
    src_cpu: i32,
    src_nid: i32,
    dst_cpu: i32,
    dst_nid: i32,
}

impl SchedStickNumaRow {
    fn new(meta: &SchedEventMeta, message: SchedStickNumaFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            pid: message.pid,
            tgid: message.tgid,
            ngid: message.ngid,
            src_cpu: message.src_cpu,
            src_nid: message.src_nid,
            dst_cpu: message.dst_cpu,
            dst_nid: message.dst_nid,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedSwapNumaRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    src_pid: i32,
    src_tgid: i32,
    src_ngid: i32,
    src_cpu: i32,
    src_nid: i32,
    dst_pid: i32,
    dst_tgid: i32,
    dst_ngid: i32,
    dst_cpu: i32,
    dst_nid: i32,
}

impl SchedSwapNumaRow {
    fn new(meta: &SchedEventMeta, message: SchedSwapNumaFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            src_pid: message.src_pid,
            src_tgid: message.src_tgid,
            src_ngid: message.src_ngid,
            src_cpu: message.src_cpu,
            src_nid: message.src_nid,
            dst_pid: message.dst_pid,
            dst_tgid: message.dst_tgid,
            dst_ngid: message.dst_ngid,
            dst_cpu: message.dst_cpu,
            dst_nid: message.dst_nid,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedSwitchRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    prev_comm: String,
    prev_pid: i32,
    prev_prio: i32,
    prev_state: u64,
    next_comm: String,
    next_pid: i32,
    next_prio: i32,
}

impl SchedSwitchRow {
    fn new(meta: &SchedEventMeta, message: SchedSwitchFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            prev_comm: message.prev_comm,
            prev_pid: message.prev_pid,
            prev_prio: message.prev_prio,
            prev_state: message.prev_state,
            next_comm: message.next_comm,
            next_pid: message.next_pid,
            next_prio: message.next_prio,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedWaitTaskRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    comm: String,
    pid: i32,
    prio: i32,
}

impl SchedWaitTaskRow {
    fn new(meta: &SchedEventMeta, message: SchedWaitTaskFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            comm: message.comm,
            pid: message.pid,
            prio: message.prio,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedWakeIdleWithoutIpiRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    cpu: i32,
}

impl SchedWakeIdleWithoutIpiRow {
    fn new(meta: &SchedEventMeta, message: SchedWakeIdleWithoutIpiFormat) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            cpu: message.cpu,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SchedWakeupRow {
    event_timestamp: u64,
    event_cpu: u32,
    event_tgid: i32,
    event_comm: String,
    comm: String,
    pid: i32,
    prio: i32,
    success: i32,
    target_cpu: i32,
}

impl SchedWakeupRow {
    fn from_wakeup(meta: &SchedEventMeta, message: SchedWakeupFormat) -> Self {
        Self::new(
            meta,
            message.comm,
            message.pid,
            message.prio,
            message.success,
            message.target_cpu,
        )
    }

    fn from_wakeup_new(meta: &SchedEventMeta, message: SchedWakeupNewFormat) -> Self {
        Self::new(
            meta,
            message.comm,
            message.pid,
            message.prio,
            message.success,
            message.target_cpu,
        )
    }

    fn from_waking(meta: &SchedEventMeta, message: SchedWakingFormat) -> Self {
        Self::new(
            meta,
            message.comm,
            message.pid,
            message.prio,
            message.success,
            message.target_cpu,
        )
    }

    fn new(
        meta: &SchedEventMeta,
        comm: String,
        pid: i32,
        prio: i32,
        success: i32,
        target_cpu: i32,
    ) -> Self {
        Self {
            event_timestamp: meta.event_timestamp,
            event_cpu: meta.event_cpu,
            event_tgid: meta.event_tgid,
            event_comm: meta.event_comm.clone(),
            comm,
            pid,
            prio,
            success,
            target_cpu,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ThreadStateRow {
    ts: u64,
    dur: Option<u64>,
    cpu: Option<u32>,
    tid: i32,
    state: String,
    comm: String,
}

#[derive(Default)]
struct ThreadStateBuilder {
    rows: Vec<ThreadStateRow>,
    active_by_tid: HashMap<i32, usize>,
}

impl ThreadStateBuilder {
    fn push_switch(&mut self, row: &SchedSwitchRow) {
        if row.prev_pid != 0 {
            self.push_state(
                row.event_timestamp,
                None,
                row.prev_pid,
                format!("prev_state:{}", row.prev_state),
                row.prev_comm.clone(),
            );
        }
        if row.next_pid != 0 {
            self.push_state(
                row.event_timestamp,
                Some(row.event_cpu),
                row.next_pid,
                "Running".to_string(),
                row.next_comm.clone(),
            );
        }
    }

    fn push_state(&mut self, ts: u64, cpu: Option<u32>, tid: i32, state: String, comm: String) {
        if let Some(active_row) = self.active_by_tid.insert(tid, self.rows.len()) {
            let start_ts = self.rows[active_row].ts;
            if ts >= start_ts {
                self.rows[active_row].dur = Some(ts - start_ts);
            }
        }

        self.rows.push(ThreadStateRow {
            ts,
            dur: None,
            cpu,
            tid,
            state,
            comm,
        });
    }

    fn into_rows(self) -> Vec<ThreadStateRow> {
        self.rows
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct InstantRow {
    ts: u64,
    name: String,
    #[serde(rename = "ref")]
    ref_tid: i32,
    wakeup_from: i32,
    ref_type: String,
    value: f64,
}

impl InstantRow {
    fn from_wakeup(row: &SchedWakeupRow, name: &str) -> Self {
        Self {
            ts: row.event_timestamp,
            name: name.to_string(),
            ref_tid: row.pid,
            wakeup_from: row.event_tgid,
            ref_type: "tid".to_string(),
            value: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ProfilerSection {
    start: usize,
    end: usize,
    len: usize,
    data_type: u32,
}

impl ProfilerSection {
    fn body<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        &bytes[self.start + PROFILER_HEADER_SIZE..self.end]
    }
}

fn read_profiler_section(bytes: &[u8], offset: usize) -> Result<ProfilerSection> {
    ensure_available(bytes, offset, PROFILER_HEADER_SIZE, "profiler header")?;

    let magic = read_u64_le(bytes, offset)?;
    if magic != PROFILER_HEADER_MAGIC {
        bail!("invalid profiler header magic at byte {offset}: 0x{magic:x}");
    }

    let len = usize::try_from(read_u64_le(bytes, offset + 8)?)
        .with_context(|| format!("invalid profiler section length at byte {offset}"))?;
    let data_type = read_u32_le(bytes, offset + 56)?;
    let Some(end) = offset.checked_add(len) else {
        bail!("invalid profiler section length {len} at byte {offset}");
    };
    if len < PROFILER_HEADER_SIZE || end > bytes.len() {
        bail!("invalid profiler section length {len} at byte {offset}");
    }

    Ok(ProfilerSection {
        start: offset,
        end,
        len,
        data_type,
    })
}

fn decode_len_prefixed_messages<T>(bytes: &[u8]) -> Result<Vec<T>>
where
    T: Message + Default,
{
    let mut offset = 0usize;
    let mut messages = Vec::new();

    while offset < bytes.len() {
        ensure_available(bytes, offset, SEGMENT_LENGTH_SIZE, "segment length")?;
        let len = read_u32_le(bytes, offset)? as usize;
        offset += SEGMENT_LENGTH_SIZE;
        ensure_available(bytes, offset, len, "profiler segment")?;

        let segment = &bytes[offset..offset + len];
        let message =
            T::decode(segment).context("failed to decode length-prefixed protobuf message")?;
        messages.push(message);
        offset += len;
    }

    Ok(messages)
}

fn ensure_available(bytes: &[u8], offset: usize, len: usize, context: &str) -> Result<()> {
    if bytes.len().saturating_sub(offset) < len {
        bail!("truncated {context} at byte {offset}");
    }

    Ok(())
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32> {
    ensure_available(bytes, offset, 4, "u32")?;
    Ok(u32::from_le_bytes(bytes[offset..offset + 4].try_into()?))
}

fn read_u64_le(bytes: &[u8], offset: usize) -> Result<u64> {
    ensure_available(bytes, offset, 8, "u64")?;
    Ok(u64::from_le_bytes(bytes[offset..offset + 8].try_into()?))
}
