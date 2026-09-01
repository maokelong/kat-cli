use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::proto::ftrace2parquet::{TextFtraceEvent, text_ftrace_event::Payload};
use crate::relation_writer::RelationWriter;

use super::{header::FtraceHeader, writer::TableWriter};

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
struct HeaderRow {
    tracer: String,
    entries_in_buffer: u64,
    entries_written: u64,
    cpu_count: u32,
    has_tgid_column: bool,
}

pub(crate) struct OutputTables {
    relations: RelationWriter,
    occurrence: Option<TableWriter<OccurrenceRow>>,
    root: Option<TableWriter<RootRow>>,
    sched_switch: Option<TableWriter<SchedSwitchRow>>,
    sched_wakeup: Option<TableWriter<WakeupRow>>,
    sched_wakeup_new: Option<TableWriter<WakeupRow>>,
    tracing_mark_write: Option<TableWriter<TracingMarkWriteRow>>,
    header: Option<FtraceHeader>,
    next_root: u64,
    next_switch: u64,
    next_wakeup: u64,
    next_wakeup_new: u64,
    next_marker: u64,
}

impl OutputTables {
    pub(crate) fn new(directory: &Path) -> Self {
        Self {
            relations: RelationWriter::new(directory),
            occurrence: None,
            root: None,
            sched_switch: None,
            sched_wakeup: None,
            sched_wakeup_new: None,
            tracing_mark_write: None,
            header: None,
            next_root: 0,
            next_switch: 0,
            next_wakeup: 0,
            next_wakeup_new: 0,
            next_marker: 0,
        }
    }

    pub(crate) fn set_header(&mut self, header: FtraceHeader) {
        self.header = Some(header);
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

    pub(crate) fn finish(self) -> Result<()> {
        let header = self.header.context("validated ftrace header is missing")?;
        let mut header_table =
            TableWriter::<HeaderRow>::new(&self.relations, "text_ftrace_header")?;
        header_table.push(HeaderRow {
            tracer: header.tracer,
            entries_in_buffer: header.entries_in_buffer,
            entries_written: header.entries_written,
            cpu_count: header.cpu_count,
            has_tgid_column: header.has_tgid_column,
        })?;
        header_table.finish()?;
        finish(self.occurrence)?;
        finish(self.root)?;
        finish(self.sched_switch)?;
        finish(self.sched_wakeup)?;
        finish(self.sched_wakeup_new)?;
        finish(self.tracing_mark_write)?;
        self.relations.validate()?;
        Ok(())
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
