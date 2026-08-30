mod claim;
mod envelope;
mod framing;
mod payload;
mod registry;

use anyhow::{Context, Result};

use crate::{
    proto::ProfilerPluginData,
    record::{TraceRecord, TraceRecordSink},
};

pub(crate) use envelope::{PluginEnvelope, PluginEnvelopeKind};
pub(crate) use framing::for_each_profiler_envelope_frame;
pub(crate) use payload::decode_payload;
pub(crate) use registry::{PluginDecoder, PluginDecoderSpec, PluginPayloadRegistry};

pub(crate) use claim::{PluginPayloadClaimant, dispatch_plugin_envelope};

pub(crate) fn decode_plugin_section_body(
    section_body: &[u8],
    section_start: usize,
    registry: &mut PluginPayloadRegistry,
    sink: &mut impl TraceRecordSink,
) -> Result<()> {
    decode_plugin_section_body_with_observer(
        section_body,
        section_start,
        registry,
        sink,
        |_message, _known, _frame_offset| Ok(()),
    )
}

pub(crate) fn decode_plugin_section_body_with_observer(
    section_body: &[u8],
    section_start: usize,
    registry: &mut PluginPayloadRegistry,
    sink: &mut impl TraceRecordSink,
    observer: impl FnMut(&ProfilerPluginData, bool, usize) -> Result<()>,
) -> Result<()> {
    let mut claimant = |_envelope: &PluginEnvelope<'_>| Ok(false);
    decode_plugin_section_body_with_claimant_and_observer(
        section_body,
        section_start,
        registry,
        sink,
        &mut claimant,
        observer,
    )
}

pub(crate) fn decode_plugin_section_body_with_claimant_and_observer(
    section_body: &[u8],
    section_start: usize,
    registry: &mut PluginPayloadRegistry,
    sink: &mut impl TraceRecordSink,
    claimant: &mut (impl PluginPayloadClaimant + ?Sized),
    mut observer: impl FnMut(&ProfilerPluginData, bool, usize) -> Result<()>,
) -> Result<()> {
    for_each_profiler_envelope_frame(section_body, |message, frame_offset| {
        {
            let envelope =
                PluginEnvelope::from_profiler_plugin_data(&message, section_start + frame_offset);
            let known = dispatch_plugin_envelope(&envelope, registry, sink, claimant)?;
            observer(&message, known, frame_offset)?;
        }
        sink.push(TraceRecord::ProfilerPluginData(message))
            .with_context(|| {
                format!("failed to append profiler section at byte {section_start} to trace sink")
            })?;
        Ok(())
    })
    .with_context(|| format!("failed to parse profiler section at byte {section_start}"))
}
