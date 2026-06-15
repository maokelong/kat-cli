// Arrow sink 是当前唯一表物化实现，负责把记录流转换为可注册查询表。

mod table_builder;

use anyhow::Result;

use crate::{
    catalog::{TableCategory, TraceDataset},
    ftrace_event_table_builders::FtraceEventTableBuilders,
    proto::ProfilerPluginData,
    record::{TraceRecord, TraceRecordSink},
};

use table_builder::TableBuilder;
pub(crate) use table_builder::{DirectEventTableBuilder, EventMeta};

const PROFILER_PLUGIN_DATA_TABLE: &str = "profiler_plugin_data";

pub(crate) struct ArrowSink {
    profiler_table: TableBuilder<ProfilerPluginData>,
    event_tables: FtraceEventTableBuilders,
}

impl ArrowSink {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            profiler_table: TableBuilder::new(PROFILER_PLUGIN_DATA_TABLE)?,
            event_tables: FtraceEventTableBuilders::new()?,
        })
    }

    pub(crate) fn finish(self) -> Result<TraceDataset> {
        let mut tables = vec![self.profiler_table.into_table(TableCategory::Raw)?];
        tables.extend(self.event_tables.into_tables()?);

        Ok(TraceDataset::new(tables))
    }
}

impl TraceRecordSink for ArrowSink {
    fn push(&mut self, record: TraceRecord) -> Result<()> {
        match record {
            TraceRecord::ProfilerPluginData(message) => {
                self.profiler_table.push(message)?;
            }
            TraceRecord::FtraceEvent(event) => self.event_tables.push_event(*event)?,
        }

        Ok(())
    }
}
