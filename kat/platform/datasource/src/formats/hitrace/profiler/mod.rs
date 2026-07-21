mod envelope;
mod framing;
mod payload;
mod registry;

use anyhow::{Context, Result};

use crate::record::{TraceRecord, TraceRecordSink};

pub(crate) use envelope::{PluginEnvelope, PluginEnvelopeKind};
pub(crate) use payload::decode_payload;
pub(crate) use registry::{PluginDecoder, PluginDecoderSpec, PluginPayloadRegistry};

use framing::for_each_profiler_envelope_frame;

pub(crate) fn decode_plugin_section_body(
    section_body: &[u8],
    section_start: usize,
    registry: &mut PluginPayloadRegistry,
    sink: &mut impl TraceRecordSink,
) -> Result<()> {
    for_each_profiler_envelope_frame(section_body, |message| {
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
