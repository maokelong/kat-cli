// Pre-sink trace record stream shared by format/domain decoders and sinks.

use anyhow::Result;

use crate::{
    domains::{
        fixed_result::FixedResultRecord, ftrace::FtraceRecord, native_hook::NativeHookRecord,
    },
    proto::ProfilerPluginData,
};

pub(crate) enum TraceRecord {
    ProfilerPluginData(ProfilerPluginData),
    Ftrace(Box<FtraceRecord>),
    NativeHook(Box<NativeHookRecord>),
    FixedResult(Box<FixedResultRecord>),
}

pub(crate) trait TraceRecordSink {
    fn push(&mut self, record: TraceRecord) -> Result<()>;
}
