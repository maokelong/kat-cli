// Trace dataset catalog shared by decoders, sinks, and query registration.

use anyhow::Result;
use arrow_array::RecordBatch;

use crate::{domains::ftrace::FtraceEventRecord, proto::ProfilerPluginData};

pub(crate) const PROFILER_PLUGIN_DATA_TABLE: &str = "profiler_plugin_data";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TableCategory {
    Raw,
    DirectEvent,
}

pub(crate) struct TraceTable {
    pub(crate) name: &'static str,
    pub(crate) category: TableCategory,
    pub(crate) batches: Vec<RecordBatch>,
}

impl TraceTable {
    pub(crate) fn new(
        name: &'static str,
        category: TableCategory,
        batches: Vec<RecordBatch>,
    ) -> Self {
        Self {
            name,
            category,
            batches,
        }
    }
}

pub(crate) struct TraceDataset {
    pub(crate) tables: Vec<TraceTable>,
}

impl TraceDataset {
    pub(crate) fn new(tables: Vec<TraceTable>) -> Self {
        Self { tables }
    }
}

pub(crate) enum TraceRecord {
    ProfilerPluginData(ProfilerPluginData),
    FtraceEvent(Box<FtraceEventRecord>),
}

pub(crate) trait TraceRecordSink {
    fn push(&mut self, record: TraceRecord) -> Result<()>;
}
