// 解析阶段的中立记录流连接格式/domain decoder 与 sink，避免上游直接绑定表实现。

use anyhow::Result;

use crate::{domains::ftrace::FtraceEventRecord, proto::ProfilerPluginData};

pub(crate) enum TraceRecord {
    ProfilerPluginData(ProfilerPluginData),
    FtraceEvent(Box<FtraceEventRecord>),
}

pub(crate) trait TraceRecordSink {
    fn push(&mut self, record: TraceRecord) -> Result<()>;
}
