//! Arrow sink for trace records.

mod table_builder;

use anyhow::Result;

use crate::{
    catalog::{
        PROFILER_PLUGIN_DATA_TABLE, TableCategory, TraceDataset, TraceRecord, TraceRecordSink,
    },
    ftrace_event_table_builders::FtraceEventTableBuilders,
    proto::ProfilerPluginData,
};

use table_builder::TableBuilder;
pub(crate) use table_builder::{DirectEventTableBuilder, EventMeta};

pub(crate) struct ArrowSink {
    profiler_table: TableBuilder<ProfilerPluginData>,
    profiler_table_seen: bool,
    profiler_rows: usize,
    event_tables: FtraceEventTableBuilders,
}

impl ArrowSink {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            profiler_table: TableBuilder::new(PROFILER_PLUGIN_DATA_TABLE)?,
            profiler_table_seen: false,
            profiler_rows: 0,
            event_tables: FtraceEventTableBuilders::new()?,
        })
    }

    pub(crate) fn finish(self) -> Result<TraceDataset> {
        let mut tables = Vec::new();
        if self.profiler_table_seen {
            tables.push(self.profiler_table.into_table(TableCategory::Raw)?);
        }
        tables.extend(self.event_tables.into_tables()?);

        Ok(TraceDataset::new(tables))
    }
}

impl TraceRecordSink for ArrowSink {
    fn push(&mut self, record: TraceRecord) -> Result<()> {
        match record {
            TraceRecord::ProfilerSection => {
                self.profiler_table_seen = true;
            }
            TraceRecord::ProfilerPluginData(message) => {
                self.profiler_table_seen = true;
                self.profiler_table.push(message)?;
                self.profiler_rows += 1;
            }
            TraceRecord::FtraceEvent(event) => self.event_tables.push_event(*event)?,
        }

        Ok(())
    }
}
