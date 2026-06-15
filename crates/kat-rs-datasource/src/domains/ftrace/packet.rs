use anyhow::{Context, Result};
use prost::Message;

use crate::{
    plugin_flow::PluginEnvelope,
    proto::TracePluginResult,
    record::{TraceRecord, TraceRecordSink},
};

use super::FtraceEventRecord;

pub(crate) const FTRACE_PLUGIN_NAME: &str = "ftrace-plugin";

pub(crate) fn decode_plugin_payload(
    envelope: &PluginEnvelope<'_>,
    sink: &mut dyn TraceRecordSink,
) -> Result<()> {
    let result = TracePluginResult::decode(envelope.payload).with_context(|| {
        format!(
            "failed to decode {} payload in profiler section at byte {} version={} sample_interval={}",
            envelope.envelope_name,
            envelope.section_start,
            envelope.version,
            envelope.sample_interval
        )
    })?;

    for detail in result.ftrace_cpu_detail {
        for event in detail.event {
            sink.push(TraceRecord::FtraceEvent(Box::new(FtraceEventRecord::new(
                detail.cpu, event,
            ))))?;
        }
    }

    Ok(())
}
