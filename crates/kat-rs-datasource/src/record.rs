// Pre-sink trace record stream shared by format/domain decoders and sinks.

use anyhow::Result;

use crate::{domains::ftrace::FtraceEventRecord, proto::ProfilerPluginData};

pub(crate) enum TraceRecord {
    ProfilerPluginData(ProfilerPluginData),
    FtraceEvent(Box<FtraceEventRecord>),
}

pub(crate) trait TraceRecordSink {
    fn push(&mut self, record: TraceRecord) -> Result<()>;
}
