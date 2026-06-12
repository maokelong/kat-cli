//! ftrace plugin domain pipeline for direct event tables.

mod table_builder;

use anyhow::{Context, Result};
use arrow_array::RecordBatch;
use prost::Message;

use crate::{proto::TracePluginResult, sched_table_builders::SchedDirectTableBuilders};

pub(crate) use table_builder::{DirectEventTableBuilder, EventMeta};

pub(crate) const FTRACE_PLUGIN_NAME: &str = "ftrace-plugin";

pub(crate) struct FtraceTable {
    pub(crate) name: &'static str,
    pub(crate) batches: Vec<RecordBatch>,
}

pub(crate) struct FtraceTables {
    sched_tables: SchedDirectTableBuilders,
}

impl FtraceTables {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            sched_tables: SchedDirectTableBuilders::new()?,
        })
    }

    pub(crate) fn push_plugin_payload(
        &mut self,
        payload: &[u8],
        section_start: usize,
    ) -> Result<()> {
        let result = TracePluginResult::decode(payload).with_context(|| {
            format!("failed to decode ftrace payload in profiler section at byte {section_start}")
        })?;

        for detail in result.ftrace_cpu_detail {
            for event in detail.event {
                self.sched_tables.push_event(detail.cpu, event)?;
            }
        }

        Ok(())
    }

    pub(crate) fn into_tables(self) -> Result<Vec<FtraceTable>> {
        self.sched_tables.into_tables()
    }
}
