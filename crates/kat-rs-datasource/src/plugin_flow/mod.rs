// plugin_flow 承接 profiler 插件流转：解析分段、保留 raw record 并调度已知 decoder。
mod envelope;
mod registry;
mod segment;

use anyhow::{Context, Result};

use crate::{
    proto::ProfilerPluginData,
    record::{TraceRecord, TraceRecordSink},
};

pub(crate) use envelope::{PluginEnvelope, PluginEnvelopeKind};
pub(crate) use registry::{PluginDecoder, PluginDecoderSpec, PluginPayloadRegistry};

use segment::for_each_len_prefixed_message;

pub(crate) fn decode_plugin_section_body(
    section_body: &[u8],
    section_start: usize,
    registry: &mut PluginPayloadRegistry,
    sink: &mut impl TraceRecordSink,
) -> Result<()> {
    for_each_len_prefixed_message::<ProfilerPluginData, _>(section_body, |message| {
        {
            let envelope = PluginEnvelope::from_profiler_plugin_data(&message, section_start);
            registry.dispatch(&envelope, sink)?;
        }
        sink.push(TraceRecord::ProfilerPluginData(message))
            .with_context(|| {
                format!("failed to append profiler section at byte {section_start} to trace sink")
            })?;
        Ok(())
    })
    .with_context(|| format!("failed to parse profiler section at byte {section_start}"))
}
