mod claim;
mod envelope;
mod framing;
mod payload;

use anyhow::{Context, Result};

use crate::proto::ProfilerPluginData;

pub(crate) use claim::PluginPayloadClaimant;
pub(crate) use envelope::{PluginEnvelope, PluginEnvelopeKind};
pub(crate) use payload::decode_payload;

pub(crate) fn decode_section(
    section_body: &[u8],
    section_start: usize,
    claimant: &mut impl PluginPayloadClaimant,
    mut observe: impl FnMut(&ProfilerPluginData, bool, usize) -> Result<()>,
) -> Result<()> {
    framing::for_each_profiler_envelope_frame(section_body, |message, frame_offset| {
        let envelope =
            PluginEnvelope::from_profiler_plugin_data(&message, section_start + frame_offset);
        let known = claimant.try_claim(&envelope)?;
        observe(&message, known, frame_offset)
    })
    .with_context(|| format!("failed to parse profiler section at byte {section_start}"))
}
