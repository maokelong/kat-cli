use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::proto::ftrace2parquet::{FilemapPageCache, TextFtraceEvent, text_ftrace_event::Payload};
use crate::relation_writer::RelationWriter;

use super::{MATERIALIZATION_VERSION, header::FtraceHeader, writer::TableWriter};

#[derive(Serialize, Deserialize)]
struct OccurrenceRow {
    _kat_row_id: u64,
    source_event_sequence: u64,
}

#[derive(Serialize, Deserialize)]
struct RootRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    clock_domain: String,
    clock_value: u64,
    cpu: u32,
    emitter_thread_name: String,
    emitter_thread_id: i32,
    emitter_process_id: Option<i32>,
    context_flags: String,
}

#[derive(Serialize, Deserialize)]
struct SchedSwitchRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    previous_thread_name: String,
    previous_thread_id: i32,
    previous_priority: i32,
    previous_state: String,
    next_thread_name: String,
    next_thread_id: i32,
    next_priority: i32,
}

#[derive(Serialize, Deserialize)]
struct WakeupRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    thread_name: String,
    thread_id: i32,
    priority: i32,
    target_cpu: u32,
}

#[derive(Serialize, Deserialize)]
struct TracingMarkWriteRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    content: String,
}

#[derive(Serialize, Deserialize)]
struct SchedBlockedReasonRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    pid: i32,
    io_wait: u32,
    caller: String,
}

#[derive(Serialize, Deserialize)]
struct FilemapPageCacheRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    device_major: u32,
    device_minor: u32,
    inode: u64,
    page_frame_number: u64,
    offset_bytes: u64,
    order: Option<u32>,
    page_address: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct BlockRqIssueRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    device_major: u32,
    device_minor: u32,
    rwbs: String,
    bytes: u32,
    command: String,
    sector: u64,
    sector_count: u32,
    process_name: String,
}

#[derive(Serialize, Deserialize)]
struct BlockRqCompleteRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    device_major: u32,
    device_minor: u32,
    rwbs: String,
    command: String,
    sector: u64,
    sector_count: u32,
    error: i32,
}

#[derive(Serialize, Deserialize)]
struct BinderTransactionRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    transaction_id: i32,
    destination_node_id: i32,
    destination_process_id: i32,
    destination_thread_id: i32,
    reply: i32,
    flags: u32,
    code: u32,
}

#[derive(Serialize, Deserialize)]
struct PrintRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    instruction_pointer: String,
    content: String,
}

#[derive(Serialize, Deserialize)]
struct HeaderRow {
    tracer: String,
    has_tgid_column: bool,
}

#[derive(Serialize, Deserialize)]
struct UnsupportedEventRow {
    event_name: String,
}

pub(crate) struct OutputTables {
    relations: RelationWriter,
    occurrence: Option<TableWriter<OccurrenceRow>>,
    root: Option<TableWriter<RootRow>>,
    sched_switch: Option<TableWriter<SchedSwitchRow>>,
    sched_wakeup: Option<TableWriter<WakeupRow>>,
    sched_wakeup_new: Option<TableWriter<WakeupRow>>,
    tracing_mark_write: Option<TableWriter<TracingMarkWriteRow>>,
    sched_blocked_reason: Option<TableWriter<SchedBlockedReasonRow>>,
    mm_filemap_add_to_page_cache: Option<TableWriter<FilemapPageCacheRow>>,
    mm_filemap_delete_from_page_cache: Option<TableWriter<FilemapPageCacheRow>>,
    block_rq_issue: Option<TableWriter<BlockRqIssueRow>>,
    block_rq_complete: Option<TableWriter<BlockRqCompleteRow>>,
    binder_transaction: Option<TableWriter<BinderTransactionRow>>,
    print: Option<TableWriter<PrintRow>>,
    unsupported_event: Option<TableWriter<UnsupportedEventRow>>,
    header: Option<FtraceHeader>,
    next_root: u64,
    next_switch: u64,
    next_wakeup: u64,
    next_wakeup_new: u64,
    next_marker: u64,
    next_sched_blocked_reason: u64,
    next_mm_filemap_add: u64,
    next_mm_filemap_delete: u64,
    next_block_rq_issue: u64,
    next_block_rq_complete: u64,
    next_binder_transaction: u64,
    next_print: u64,
}

impl OutputTables {
    pub(crate) fn new(directory: &Path) -> Self {
        Self {
            relations: RelationWriter::new(directory, MATERIALIZATION_VERSION),
            occurrence: None,
            root: None,
            sched_switch: None,
            sched_wakeup: None,
            sched_wakeup_new: None,
            tracing_mark_write: None,
            sched_blocked_reason: None,
            mm_filemap_add_to_page_cache: None,
            mm_filemap_delete_from_page_cache: None,
            block_rq_issue: None,
            block_rq_complete: None,
            binder_transaction: None,
            print: None,
            unsupported_event: None,
            header: None,
            next_root: 0,
            next_switch: 0,
            next_wakeup: 0,
            next_wakeup_new: 0,
            next_marker: 0,
            next_sched_blocked_reason: 0,
            next_mm_filemap_add: 0,
            next_mm_filemap_delete: 0,
            next_block_rq_issue: 0,
            next_block_rq_complete: 0,
            next_binder_transaction: 0,
            next_print: 0,
        }
    }

    pub(crate) fn set_header(&mut self, header: FtraceHeader) {
        self.header = Some(header);
    }

    pub(crate) fn push_unsupported_event(&mut self, event_name: String) -> Result<()> {
        initialize(
            &self.relations,
            &mut self.unsupported_event,
            "text_ftrace_unsupported_event",
        )?
        .push(UnsupportedEventRow { event_name })
    }

    pub(crate) fn push(
        &mut self,
        source_event_sequence: u64,
        event: TextFtraceEvent,
    ) -> Result<()> {
        let root_id = take_next(&mut self.next_root)?;
        self.occurrence()?.push(OccurrenceRow {
            _kat_row_id: root_id,
            source_event_sequence,
        })?;
        let payload = event.payload.context("supported event has no payload")?;
        self.root()?.push(RootRow {
            _kat_row_id: root_id,
            _kat_parent_row_id: root_id,
            clock_domain: event.clock_domain,
            clock_value: event.clock_value,
            cpu: event.cpu,
            emitter_thread_name: event.emitter_thread_name,
            emitter_thread_id: event.emitter_thread_id,
            emitter_process_id: event.emitter_process_id,
            context_flags: event.context_flags,
        })?;
        match payload {
            Payload::SchedSwitch(value) => {
                let id = take_next(&mut self.next_switch)?;
                self.sched_switch()?.push(SchedSwitchRow {
                    _kat_row_id: id,
                    _kat_parent_row_id: root_id,
                    previous_thread_name: value.previous_thread_name,
                    previous_thread_id: value.previous_thread_id,
                    previous_priority: value.previous_priority,
                    previous_state: value.previous_state,
                    next_thread_name: value.next_thread_name,
                    next_thread_id: value.next_thread_id,
                    next_priority: value.next_priority,
                })?;
            }
            Payload::SchedWakeup(value) => {
                let id = take_next(&mut self.next_wakeup)?;
                self.sched_wakeup()?.push(WakeupRow {
                    _kat_row_id: id,
                    _kat_parent_row_id: root_id,
                    thread_name: value.thread_name,
                    thread_id: value.thread_id,
                    priority: value.priority,
                    target_cpu: value.target_cpu,
                })?;
            }
            Payload::SchedWakeupNew(value) => {
                let id = take_next(&mut self.next_wakeup_new)?;
                self.sched_wakeup_new()?.push(WakeupRow {
                    _kat_row_id: id,
                    _kat_parent_row_id: root_id,
                    thread_name: value.thread_name,
                    thread_id: value.thread_id,
                    priority: value.priority,
                    target_cpu: value.target_cpu,
                })?;
            }
            Payload::TracingMarkWrite(value) => {
                let id = take_next(&mut self.next_marker)?;
                self.tracing_mark_write()?.push(TracingMarkWriteRow {
                    _kat_row_id: id,
                    _kat_parent_row_id: root_id,
                    content: value.content,
                })?;
            }
            Payload::SchedBlockedReason(value) => {
                let id = take_next(&mut self.next_sched_blocked_reason)?;
                self.sched_blocked_reason()?.push(SchedBlockedReasonRow {
                    _kat_row_id: id,
                    _kat_parent_row_id: root_id,
                    pid: value.pid,
                    io_wait: value.io_wait,
                    caller: value.caller,
                })?;
            }
            Payload::MmFilemapAddToPageCache(value) => {
                let id = take_next(&mut self.next_mm_filemap_add)?;
                self.mm_filemap_add_to_page_cache()?
                    .push(filemap_row(id, root_id, value))?;
            }
            Payload::MmFilemapDeleteFromPageCache(value) => {
                let id = take_next(&mut self.next_mm_filemap_delete)?;
                self.mm_filemap_delete_from_page_cache()?
                    .push(filemap_row(id, root_id, value))?;
            }
            Payload::BlockRqIssue(value) => {
                let id = take_next(&mut self.next_block_rq_issue)?;
                self.block_rq_issue()?.push(BlockRqIssueRow {
                    _kat_row_id: id,
                    _kat_parent_row_id: root_id,
                    device_major: value.device_major,
                    device_minor: value.device_minor,
                    rwbs: value.rwbs,
                    bytes: value.bytes,
                    command: value.command,
                    sector: value.sector,
                    sector_count: value.sector_count,
                    process_name: value.process_name,
                })?;
            }
            Payload::BlockRqComplete(value) => {
                let id = take_next(&mut self.next_block_rq_complete)?;
                self.block_rq_complete()?.push(BlockRqCompleteRow {
                    _kat_row_id: id,
                    _kat_parent_row_id: root_id,
                    device_major: value.device_major,
                    device_minor: value.device_minor,
                    rwbs: value.rwbs,
                    command: value.command,
                    sector: value.sector,
                    sector_count: value.sector_count,
                    error: value.error,
                })?;
            }
            Payload::BinderTransaction(value) => {
                let id = take_next(&mut self.next_binder_transaction)?;
                self.binder_transaction()?.push(BinderTransactionRow {
                    _kat_row_id: id,
                    _kat_parent_row_id: root_id,
                    transaction_id: value.transaction_id,
                    destination_node_id: value.destination_node_id,
                    destination_process_id: value.destination_process_id,
                    destination_thread_id: value.destination_thread_id,
                    reply: value.reply,
                    flags: value.flags,
                    code: value.code,
                })?;
            }
            Payload::Print(value) => {
                let id = take_next(&mut self.next_print)?;
                self.print()?.push(PrintRow {
                    _kat_row_id: id,
                    _kat_parent_row_id: root_id,
                    instruction_pointer: value.instruction_pointer,
                    content: value.content,
                })?;
            }
        }
        Ok(())
    }

    fn occurrence(&mut self) -> Result<&mut TableWriter<OccurrenceRow>> {
        initialize(
            &self.relations,
            &mut self.occurrence,
            "text_ftrace_event_occurrence",
        )
    }

    fn root(&mut self) -> Result<&mut TableWriter<RootRow>> {
        initialize(&self.relations, &mut self.root, "text_ftrace_event")
    }

    fn sched_switch(&mut self) -> Result<&mut TableWriter<SchedSwitchRow>> {
        initialize(
            &self.relations,
            &mut self.sched_switch,
            "text_ftrace_event_sched_switch",
        )
    }

    fn sched_wakeup(&mut self) -> Result<&mut TableWriter<WakeupRow>> {
        initialize(
            &self.relations,
            &mut self.sched_wakeup,
            "text_ftrace_event_sched_wakeup",
        )
    }

    fn sched_wakeup_new(&mut self) -> Result<&mut TableWriter<WakeupRow>> {
        initialize(
            &self.relations,
            &mut self.sched_wakeup_new,
            "text_ftrace_event_sched_wakeup_new",
        )
    }

    fn tracing_mark_write(&mut self) -> Result<&mut TableWriter<TracingMarkWriteRow>> {
        initialize(
            &self.relations,
            &mut self.tracing_mark_write,
            "text_ftrace_event_tracing_mark_write",
        )
    }

    fn sched_blocked_reason(&mut self) -> Result<&mut TableWriter<SchedBlockedReasonRow>> {
        initialize(
            &self.relations,
            &mut self.sched_blocked_reason,
            "text_ftrace_event_sched_blocked_reason",
        )
    }

    fn mm_filemap_add_to_page_cache(&mut self) -> Result<&mut TableWriter<FilemapPageCacheRow>> {
        initialize(
            &self.relations,
            &mut self.mm_filemap_add_to_page_cache,
            "text_ftrace_event_mm_filemap_add_to_page_cache",
        )
    }

    fn mm_filemap_delete_from_page_cache(
        &mut self,
    ) -> Result<&mut TableWriter<FilemapPageCacheRow>> {
        initialize(
            &self.relations,
            &mut self.mm_filemap_delete_from_page_cache,
            "text_ftrace_event_mm_filemap_delete_from_page_cache",
        )
    }

    fn block_rq_issue(&mut self) -> Result<&mut TableWriter<BlockRqIssueRow>> {
        initialize(
            &self.relations,
            &mut self.block_rq_issue,
            "text_ftrace_event_block_rq_issue",
        )
    }

    fn block_rq_complete(&mut self) -> Result<&mut TableWriter<BlockRqCompleteRow>> {
        initialize(
            &self.relations,
            &mut self.block_rq_complete,
            "text_ftrace_event_block_rq_complete",
        )
    }

    fn binder_transaction(&mut self) -> Result<&mut TableWriter<BinderTransactionRow>> {
        initialize(
            &self.relations,
            &mut self.binder_transaction,
            "text_ftrace_event_binder_transaction",
        )
    }

    fn print(&mut self) -> Result<&mut TableWriter<PrintRow>> {
        initialize(&self.relations, &mut self.print, "text_ftrace_event_print")
    }

    pub(crate) fn finish(self) -> Result<()> {
        let header = self.header.context("validated ftrace header is missing")?;
        let mut header_table =
            TableWriter::<HeaderRow>::new(&self.relations, "text_ftrace_header")?;
        header_table.push(HeaderRow {
            tracer: header.tracer,
            has_tgid_column: header.has_tgid_column,
        })?;
        header_table.finish()?;
        finish(self.occurrence)?;
        finish(self.root)?;
        finish(self.sched_switch)?;
        finish(self.sched_wakeup)?;
        finish(self.sched_wakeup_new)?;
        finish(self.tracing_mark_write)?;
        finish(self.sched_blocked_reason)?;
        finish(self.mm_filemap_add_to_page_cache)?;
        finish(self.mm_filemap_delete_from_page_cache)?;
        finish(self.block_rq_issue)?;
        finish(self.block_rq_complete)?;
        finish(self.binder_transaction)?;
        finish(self.print)?;
        finish(self.unsupported_event)?;
        self.relations.validate()?;
        Ok(())
    }
}

fn filemap_row(id: u64, root_id: u64, value: FilemapPageCache) -> FilemapPageCacheRow {
    FilemapPageCacheRow {
        _kat_row_id: id,
        _kat_parent_row_id: root_id,
        device_major: value.device_major,
        device_minor: value.device_minor,
        inode: value.inode,
        page_frame_number: value.page_frame_number,
        offset_bytes: value.offset_bytes,
        order: value.order,
        page_address: value.page_address,
    }
}

fn initialize<'a, T>(
    relations: &RelationWriter,
    table: &'a mut Option<TableWriter<T>>,
    name: &'static str,
) -> Result<&'a mut TableWriter<T>>
where
    for<'de> T: Deserialize<'de>,
    T: Serialize,
{
    if table.is_none() {
        *table = Some(TableWriter::new(relations, name)?);
    }
    Ok(table.as_mut().expect("table initialized"))
}

fn finish<T>(table: Option<TableWriter<T>>) -> Result<()>
where
    for<'de> T: Deserialize<'de>,
    T: Serialize,
{
    if let Some(table) = table {
        table.finish()?;
    }
    Ok(())
}

fn take_next(value: &mut u64) -> Result<u64> {
    let current = *value;
    *value = value.checked_add(1).context("row id overflows")?;
    Ok(current)
}
