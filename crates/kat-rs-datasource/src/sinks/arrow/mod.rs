//! Arrow sink for trace records.

mod ftrace;
mod native_hook;
mod table;

use anyhow::Result;

use crate::{
    catalog::{TableCategory, TraceDataset},
    ftrace_event_table_builders::FtraceTableSet,
    native_hook_table_builders::NativeHookTableSet,
    proto::ProfilerPluginData,
    record::{TraceRecord, TraceRecordSink},
};

pub(crate) use ftrace::{EventMeta, FtraceEventTableBuilder};
pub(crate) use native_hook::NativeHookEventTableBuilder;
pub(crate) use table::MessageTableBuilder;

const PROFILER_PLUGIN_DATA_TABLE: &str = "profiler_plugin_data";

pub(crate) struct ArrowSink {
    profiler_table: MessageTableBuilder<ProfilerPluginData>,
    event_tables: FtraceTableSet,
    native_hook_tables: NativeHookTableSet,
}

impl ArrowSink {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            profiler_table: MessageTableBuilder::new(PROFILER_PLUGIN_DATA_TABLE)?,
            event_tables: FtraceTableSet::new()?,
            native_hook_tables: NativeHookTableSet::new()?,
        })
    }

    pub(crate) fn finish(self) -> Result<TraceDataset> {
        let mut tables = vec![self.profiler_table.into_table(TableCategory::Raw)?];
        tables.extend(self.event_tables.into_tables()?);
        tables.extend(self.native_hook_tables.into_tables()?);

        Ok(TraceDataset::new(tables))
    }
}

impl TraceRecordSink for ArrowSink {
    fn push(&mut self, record: TraceRecord) -> Result<()> {
        match record {
            TraceRecord::ProfilerPluginData(message) => {
                self.profiler_table.push(message)?;
            }
            TraceRecord::Ftrace(record) => self.event_tables.push_record(*record)?,
            TraceRecord::NativeHook(record) => self.native_hook_tables.push_record(*record)?,
        }

        Ok(())
    }
}
