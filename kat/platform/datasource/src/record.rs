// Pre-sink trace record stream shared by format/domain decoders and sinks.

use anyhow::Result;

use crate::{
    domains::{
        ftrace::{FtraceCaptureRecord, FtraceRecord},
        native_hook::NativeHookRecord,
    },
    proto::ProfilerPluginData,
};

pub(crate) enum TraceRecord {
    ProfilerPluginData(ProfilerPluginData),
    FtraceCapture(FtraceCaptureRecord),
    Ftrace(Box<FtraceRecord>),
    NativeHook(Box<NativeHookRecord>),
}

pub(crate) trait TraceRecordSink {
    fn push(&mut self, record: TraceRecord) -> Result<()>;
}
