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
    proto::{ProfilerPluginData, TracePluginResult, kat::hitrace::FtraceEvent},
    sched_rows::{
        SchedBlockedReasonRow, SchedEventMeta, SchedKthreadStopRetRow, SchedKthreadStopRow,
        SchedMigrateTaskRow, SchedMoveNumaRow, SchedPiSetprioRow, SchedProcessExecRow,
        SchedProcessExitRow, SchedProcessForkRow, SchedProcessFreeRow, SchedProcessWaitRow,
        SchedStatBlockedRow, SchedStatIowaitRow, SchedStatRuntimeRow, SchedStatSleepRow,
        SchedStatWaitRow, SchedStickNumaRow, SchedSwapNumaRow, SchedSwitchRow, SchedWaitTaskRow,
        SchedWakeIdleWithoutIpiRow, SchedWakeupNewRow, SchedWakeupRow, SchedWakingRow,
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
    sched_stat_blocked: Vec<SchedStatBlockedRow>,
    sched_stat_iowait: Vec<SchedStatIowaitRow>,
    sched_stat_runtime: Vec<SchedStatRuntimeRow>,
    sched_stat_sleep: Vec<SchedStatSleepRow>,
    sched_stat_wait: Vec<SchedStatWaitRow>,
    sched_stick_numa: Vec<SchedStickNumaRow>,
    sched_swap_numa: Vec<SchedSwapNumaRow>,
    sched_switch: Vec<SchedSwitchRow>,
    sched_wait_task: Vec<SchedWaitTaskRow>,
    sched_wake_idle_without_ipi: Vec<SchedWakeIdleWithoutIpiRow>,
    sched_wakeup: Vec<SchedWakeupRow>,
    sched_wakeup_new: Vec<SchedWakeupNewRow>,
    sched_waking: Vec<SchedWakingRow>,
    thread_state: ThreadStateBuilder,
    instant: Vec<InstantRow>,
}

impl SchedRows {
    fn push_event(&mut self, cpu: u32, event: FtraceEvent) {
        let meta = SchedEventMeta::from_event(cpu, &event);

        if let Some(message) = event.sched_kthread_stop_format {
            self.sched_kthread_stop
                .push(SchedKthreadStopRow::new(&meta, message));
        }
        if let Some(message) = event.sched_kthread_stop_ret_format {
            self.sched_kthread_stop_ret
                .push(SchedKthreadStopRetRow::new(&meta, message));
        }
        if let Some(message) = event.sched_migrate_task_format {
            self.sched_migrate_task
                .push(SchedMigrateTaskRow::new(&meta, message));
        }
        if let Some(message) = event.sched_move_numa_format {
            self.sched_move_numa
                .push(SchedMoveNumaRow::new(&meta, message));
        }
        if let Some(message) = event.sched_pi_setprio_format {
            self.sched_pi_setprio
                .push(SchedPiSetprioRow::new(&meta, message));
        }
        if let Some(message) = event.sched_process_exec_format {
            self.sched_process_exec
                .push(SchedProcessExecRow::new(&meta, message));
        }
        if let Some(message) = event.sched_process_exit_format {
            self.sched_process_exit
                .push(SchedProcessExitRow::new(&meta, message));
        }
        if let Some(message) = event.sched_process_fork_format {
            self.sched_process_fork
                .push(SchedProcessForkRow::new(&meta, message));
        }
        if let Some(message) = event.sched_process_free_format {
            self.sched_process_free
                .push(SchedProcessFreeRow::new(&meta, message));
        }
        if let Some(message) = event.sched_process_wait_format {
            self.sched_process_wait
                .push(SchedProcessWaitRow::new(&meta, message));
        }
        if let Some(message) = event.sched_stat_blocked_format {
            self.sched_stat_blocked
                .push(SchedStatBlockedRow::new(&meta, message));
        }
        if let Some(message) = event.sched_stat_iowait_format {
            self.sched_stat_iowait
                .push(SchedStatIowaitRow::new(&meta, message));
        }
        if let Some(message) = event.sched_stat_runtime_format {
            self.sched_stat_runtime
                .push(SchedStatRuntimeRow::new(&meta, message));
        }
        if let Some(message) = event.sched_stat_sleep_format {
            self.sched_stat_sleep
                .push(SchedStatSleepRow::new(&meta, message));
        }
        if let Some(message) = event.sched_stat_wait_format {
            self.sched_stat_wait
                .push(SchedStatWaitRow::new(&meta, message));
        }
        if let Some(message) = event.sched_stick_numa_format {
            self.sched_stick_numa
                .push(SchedStickNumaRow::new(&meta, message));
        }
        if let Some(message) = event.sched_swap_numa_format {
            self.sched_swap_numa
                .push(SchedSwapNumaRow::new(&meta, message));
        }
        if let Some(message) = event.sched_switch_format {
            let row = SchedSwitchRow::new(&meta, message);
            self.thread_state.push_switch(&row);
            self.sched_switch.push(row);
        }
        if let Some(message) = event.sched_wait_task_format {
            self.sched_wait_task
                .push(SchedWaitTaskRow::new(&meta, message));
        }
        if let Some(message) = event.sched_wake_idle_without_ipi_format {
            self.sched_wake_idle_without_ipi
                .push(SchedWakeIdleWithoutIpiRow::new(&meta, message));
        }
        if let Some(message) = event.sched_wakeup_format {
            let row = SchedWakeupRow::new(&meta, message);
            self.instant.push(InstantRow::from_wakeup(
                row.event_timestamp,
                "sched_wakeup",
                row.pid,
                row.event_tgid,
            ));
            self.sched_wakeup.push(row);
        }
        if let Some(message) = event.sched_wakeup_new_format {
            let row = SchedWakeupNewRow::new(&meta, message);
            self.instant.push(InstantRow::from_wakeup(
                row.event_timestamp,
                "sched_wakeup_new",
                row.pid,
                row.event_tgid,
            ));
            self.sched_wakeup_new.push(row);
        }
        if let Some(message) = event.sched_waking_format {
            let row = SchedWakingRow::new(&meta, message);
            self.instant.push(InstantRow::from_wakeup(
                row.event_timestamp,
                "sched_waking",
                row.pid,
                row.event_tgid,
            ));
            self.sched_waking.push(row);
        }
        if let Some(message) = event.sched_blocked_reason_format {
            self.sched_blocked_reason
                .push(SchedBlockedReasonRow::new(&meta, message));
        }
    }

    fn into_tables(self) -> Result<Vec<HitraceTable>> {
        Ok(vec![
            table_from_rows(SchedBlockedReasonRow::TABLE_NAME, self.sched_blocked_reason)?,
            table_from_rows(SchedKthreadStopRow::TABLE_NAME, self.sched_kthread_stop)?,
            table_from_rows(
                SchedKthreadStopRetRow::TABLE_NAME,
                self.sched_kthread_stop_ret,
            )?,
            table_from_rows(SchedMigrateTaskRow::TABLE_NAME, self.sched_migrate_task)?,
            table_from_rows(SchedMoveNumaRow::TABLE_NAME, self.sched_move_numa)?,
            table_from_rows(SchedPiSetprioRow::TABLE_NAME, self.sched_pi_setprio)?,
            table_from_rows(SchedProcessExecRow::TABLE_NAME, self.sched_process_exec)?,
            table_from_rows(SchedProcessExitRow::TABLE_NAME, self.sched_process_exit)?,
            table_from_rows(SchedProcessForkRow::TABLE_NAME, self.sched_process_fork)?,
            table_from_rows(SchedProcessFreeRow::TABLE_NAME, self.sched_process_free)?,
            table_from_rows(SchedProcessWaitRow::TABLE_NAME, self.sched_process_wait)?,
            table_from_rows(SchedStatBlockedRow::TABLE_NAME, self.sched_stat_blocked)?,
            table_from_rows(SchedStatIowaitRow::TABLE_NAME, self.sched_stat_iowait)?,
            table_from_rows(SchedStatRuntimeRow::TABLE_NAME, self.sched_stat_runtime)?,
            table_from_rows(SchedStatSleepRow::TABLE_NAME, self.sched_stat_sleep)?,
            table_from_rows(SchedStatWaitRow::TABLE_NAME, self.sched_stat_wait)?,
            table_from_rows(SchedStickNumaRow::TABLE_NAME, self.sched_stick_numa)?,
            table_from_rows(SchedSwapNumaRow::TABLE_NAME, self.sched_swap_numa)?,
            table_from_rows(SchedSwitchRow::TABLE_NAME, self.sched_switch)?,
            table_from_rows(SchedWaitTaskRow::TABLE_NAME, self.sched_wait_task)?,
            table_from_rows(
                SchedWakeIdleWithoutIpiRow::TABLE_NAME,
                self.sched_wake_idle_without_ipi,
            )?,
            table_from_rows(SchedWakeupRow::TABLE_NAME, self.sched_wakeup)?,
            table_from_rows(SchedWakeupNewRow::TABLE_NAME, self.sched_wakeup_new)?,
            table_from_rows(SchedWakingRow::TABLE_NAME, self.sched_waking)?,
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
    fn from_wakeup(ts: u64, name: &str, ref_tid: i32, wakeup_from: i32) -> Self {
        Self {
            ts,
            name: name.to_string(),
            ref_tid,
            wakeup_from,
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
