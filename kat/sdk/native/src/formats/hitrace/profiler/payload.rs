use anyhow::{Context, Result};
use prost::Message;

use super::PluginEnvelope;

pub(crate) fn decode_payload<M>(envelope: &PluginEnvelope<'_>) -> Result<M>
where
    M: Message + Default,
{
    M::decode(envelope.payload).with_context(|| {
        format!(
            "failed to decode {} payload in profiler section at byte {} version={} sample_interval={}",
            envelope.envelope_name,
            envelope.section_start,
            envelope.version,
            envelope.sample_interval
        )
    })
}
