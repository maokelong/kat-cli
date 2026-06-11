//! Parses hitrace files into Arrow batches backed by profiler plugin segments.

mod derived;

use std::{marker::PhantomData, path::Path};

use anyhow::{Context, Result, bail};
use arrow_array::RecordBatch;
use log::debug;
use prost::Message;
use serde::{Deserialize, Serialize};
use serde_arrow::{
    ArrayBuilder,
    schema::{SchemaLike, TracingOptions},
};

use crate::{
    hitrace::derived::{INSTANT_TABLE, InstantRow, THREAD_STATE_TABLE, ThreadStateBuilder},
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
    let mut sched_rows = SchedRows::new()?;

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
                sched_rows.push_event(detail.cpu, event)?;
            }
        }
    }

    Ok(())
}

struct SchedRows {
    sched_blocked_reason: TableBuilder<SchedBlockedReasonRow>,
    sched_kthread_stop: TableBuilder<SchedKthreadStopRow>,
    sched_kthread_stop_ret: TableBuilder<SchedKthreadStopRetRow>,
    sched_migrate_task: TableBuilder<SchedMigrateTaskRow>,
    sched_move_numa: TableBuilder<SchedMoveNumaRow>,
    sched_pi_setprio: TableBuilder<SchedPiSetprioRow>,
    sched_process_exec: TableBuilder<SchedProcessExecRow>,
    sched_process_exit: TableBuilder<SchedProcessExitRow>,
    sched_process_fork: TableBuilder<SchedProcessForkRow>,
    sched_process_free: TableBuilder<SchedProcessFreeRow>,
    sched_process_wait: TableBuilder<SchedProcessWaitRow>,
    sched_stat_blocked: TableBuilder<SchedStatBlockedRow>,
    sched_stat_iowait: TableBuilder<SchedStatIowaitRow>,
    sched_stat_runtime: TableBuilder<SchedStatRuntimeRow>,
    sched_stat_sleep: TableBuilder<SchedStatSleepRow>,
    sched_stat_wait: TableBuilder<SchedStatWaitRow>,
    sched_stick_numa: TableBuilder<SchedStickNumaRow>,
    sched_swap_numa: TableBuilder<SchedSwapNumaRow>,
    sched_switch: TableBuilder<SchedSwitchRow>,
    sched_wait_task: TableBuilder<SchedWaitTaskRow>,
    sched_wake_idle_without_ipi: TableBuilder<SchedWakeIdleWithoutIpiRow>,
    sched_wakeup: TableBuilder<SchedWakeupRow>,
    sched_wakeup_new: TableBuilder<SchedWakeupNewRow>,
    sched_waking: TableBuilder<SchedWakingRow>,
    thread_state: ThreadStateBuilder,
    instant: Vec<InstantRow>,
}

impl SchedRows {
    fn new() -> Result<Self> {
        Ok(Self {
            sched_blocked_reason: TableBuilder::new(SchedBlockedReasonRow::TABLE_NAME)?,
            sched_kthread_stop: TableBuilder::new(SchedKthreadStopRow::TABLE_NAME)?,
            sched_kthread_stop_ret: TableBuilder::new(SchedKthreadStopRetRow::TABLE_NAME)?,
            sched_migrate_task: TableBuilder::new(SchedMigrateTaskRow::TABLE_NAME)?,
            sched_move_numa: TableBuilder::new(SchedMoveNumaRow::TABLE_NAME)?,
            sched_pi_setprio: TableBuilder::new(SchedPiSetprioRow::TABLE_NAME)?,
            sched_process_exec: TableBuilder::new(SchedProcessExecRow::TABLE_NAME)?,
            sched_process_exit: TableBuilder::new(SchedProcessExitRow::TABLE_NAME)?,
            sched_process_fork: TableBuilder::new(SchedProcessForkRow::TABLE_NAME)?,
            sched_process_free: TableBuilder::new(SchedProcessFreeRow::TABLE_NAME)?,
            sched_process_wait: TableBuilder::new(SchedProcessWaitRow::TABLE_NAME)?,
            sched_stat_blocked: TableBuilder::new(SchedStatBlockedRow::TABLE_NAME)?,
            sched_stat_iowait: TableBuilder::new(SchedStatIowaitRow::TABLE_NAME)?,
            sched_stat_runtime: TableBuilder::new(SchedStatRuntimeRow::TABLE_NAME)?,
            sched_stat_sleep: TableBuilder::new(SchedStatSleepRow::TABLE_NAME)?,
            sched_stat_wait: TableBuilder::new(SchedStatWaitRow::TABLE_NAME)?,
            sched_stick_numa: TableBuilder::new(SchedStickNumaRow::TABLE_NAME)?,
            sched_swap_numa: TableBuilder::new(SchedSwapNumaRow::TABLE_NAME)?,
            sched_switch: TableBuilder::new(SchedSwitchRow::TABLE_NAME)?,
            sched_wait_task: TableBuilder::new(SchedWaitTaskRow::TABLE_NAME)?,
            sched_wake_idle_without_ipi: TableBuilder::new(SchedWakeIdleWithoutIpiRow::TABLE_NAME)?,
            sched_wakeup: TableBuilder::new(SchedWakeupRow::TABLE_NAME)?,
            sched_wakeup_new: TableBuilder::new(SchedWakeupNewRow::TABLE_NAME)?,
            sched_waking: TableBuilder::new(SchedWakingRow::TABLE_NAME)?,
            thread_state: ThreadStateBuilder::default(),
            instant: Vec::new(),
        })
    }

    fn push_event(&mut self, cpu: u32, event: FtraceEvent) -> Result<()> {
        let meta = SchedEventMeta::from_event(cpu, &event);

        if let Some(message) = event.sched_kthread_stop_format {
            self.sched_kthread_stop
                .push(SchedKthreadStopRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_kthread_stop_ret_format {
            self.sched_kthread_stop_ret
                .push(SchedKthreadStopRetRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_migrate_task_format {
            self.sched_migrate_task
                .push(SchedMigrateTaskRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_move_numa_format {
            self.sched_move_numa
                .push(SchedMoveNumaRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_pi_setprio_format {
            self.sched_pi_setprio
                .push(SchedPiSetprioRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_process_exec_format {
            self.sched_process_exec
                .push(SchedProcessExecRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_process_exit_format {
            self.sched_process_exit
                .push(SchedProcessExitRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_process_fork_format {
            self.sched_process_fork
                .push(SchedProcessForkRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_process_free_format {
            self.sched_process_free
                .push(SchedProcessFreeRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_process_wait_format {
            self.sched_process_wait
                .push(SchedProcessWaitRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_stat_blocked_format {
            self.sched_stat_blocked
                .push(SchedStatBlockedRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_stat_iowait_format {
            self.sched_stat_iowait
                .push(SchedStatIowaitRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_stat_runtime_format {
            self.sched_stat_runtime
                .push(SchedStatRuntimeRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_stat_sleep_format {
            self.sched_stat_sleep
                .push(SchedStatSleepRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_stat_wait_format {
            self.sched_stat_wait
                .push(SchedStatWaitRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_stick_numa_format {
            self.sched_stick_numa
                .push(SchedStickNumaRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_swap_numa_format {
            self.sched_swap_numa
                .push(SchedSwapNumaRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_switch_format {
            let row = SchedSwitchRow::new(&meta, message);
            self.thread_state.push_switch(&row);
            self.sched_switch.push(row)?;
        }
        if let Some(message) = event.sched_wait_task_format {
            self.sched_wait_task
                .push(SchedWaitTaskRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_wake_idle_without_ipi_format {
            self.sched_wake_idle_without_ipi
                .push(SchedWakeIdleWithoutIpiRow::new(&meta, message))?;
        }
        if let Some(message) = event.sched_wakeup_format {
            let row = SchedWakeupRow::new(&meta, message);
            self.instant.push(InstantRow::from_wakeup(
                row.event_timestamp,
                "sched_wakeup",
                row.pid,
                row.event_tgid,
            ));
            self.sched_wakeup.push(row)?;
        }
        if let Some(message) = event.sched_wakeup_new_format {
            let row = SchedWakeupNewRow::new(&meta, message);
            self.instant.push(InstantRow::from_wakeup(
                row.event_timestamp,
                "sched_wakeup_new",
                row.pid,
                row.event_tgid,
            ));
            self.sched_wakeup_new.push(row)?;
        }
        if let Some(message) = event.sched_waking_format {
            let row = SchedWakingRow::new(&meta, message);
            self.instant.push(InstantRow::from_wakeup(
                row.event_timestamp,
                "sched_waking",
                row.pid,
                row.event_tgid,
            ));
            self.sched_waking.push(row)?;
        }
        if let Some(message) = event.sched_blocked_reason_format {
            self.sched_blocked_reason
                .push(SchedBlockedReasonRow::new(&meta, message))?;
        }

        Ok(())
    }

    fn into_tables(self) -> Result<Vec<HitraceTable>> {
        Ok(vec![
            self.sched_blocked_reason.into_table()?,
            self.sched_kthread_stop.into_table()?,
            self.sched_kthread_stop_ret.into_table()?,
            self.sched_migrate_task.into_table()?,
            self.sched_move_numa.into_table()?,
            self.sched_pi_setprio.into_table()?,
            self.sched_process_exec.into_table()?,
            self.sched_process_exit.into_table()?,
            self.sched_process_fork.into_table()?,
            self.sched_process_free.into_table()?,
            self.sched_process_wait.into_table()?,
            self.sched_stat_blocked.into_table()?,
            self.sched_stat_iowait.into_table()?,
            self.sched_stat_runtime.into_table()?,
            self.sched_stat_sleep.into_table()?,
            self.sched_stat_wait.into_table()?,
            self.sched_stick_numa.into_table()?,
            self.sched_swap_numa.into_table()?,
            self.sched_switch.into_table()?,
            self.sched_wait_task.into_table()?,
            self.sched_wake_idle_without_ipi.into_table()?,
            self.sched_wakeup.into_table()?,
            self.sched_wakeup_new.into_table()?,
            self.sched_waking.into_table()?,
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

struct TableBuilder<T> {
    name: &'static str,
    builder: ArrayBuilder,
    _row: PhantomData<T>,
}

impl<T> TableBuilder<T>
where
    T: Serialize,
    for<'de> T: Deserialize<'de>,
{
    fn new(name: &'static str) -> Result<Self> {
        let fields = Vec::<arrow_schema::FieldRef>::from_type::<T>(TracingOptions::default())?;
        Ok(Self {
            name,
            builder: ArrayBuilder::from_arrow(&fields)?,
            _row: PhantomData,
        })
    }

    fn push(&mut self, row: T) -> Result<()> {
        self.builder.push(row)?;
        Ok(())
    }

    fn into_table(self) -> Result<HitraceTable> {
        let name = self.name;
        Ok(HitraceTable {
            name,
            batches: vec![
                self.builder
                    .into_record_batch()
                    .with_context(|| format!("failed to convert {name} table to Arrow"))?,
            ],
        })
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
