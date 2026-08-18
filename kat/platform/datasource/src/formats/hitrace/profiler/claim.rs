use anyhow::Result;

use crate::record::TraceRecordSink;

use super::{PluginEnvelope, PluginPayloadRegistry};

pub(crate) trait PluginPayloadClaimant {
    fn try_claim(&mut self, envelope: &PluginEnvelope<'_>) -> Result<bool>;
}

impl<F> PluginPayloadClaimant for F
where
    F: FnMut(&PluginEnvelope<'_>) -> Result<bool>,
{
    fn try_claim(&mut self, envelope: &PluginEnvelope<'_>) -> Result<bool> {
        self(envelope)
    }
}

pub(crate) fn dispatch_plugin_envelope(
    envelope: &PluginEnvelope<'_>,
    registry: &mut PluginPayloadRegistry,
    sink: &mut dyn TraceRecordSink,
    claimant: &mut (impl PluginPayloadClaimant + ?Sized),
) -> Result<bool> {
    // Claim 必须发生在旧 decoder 之前；成功 claim 后旧 Native Hook records 不再产生。
    if claimant.try_claim(envelope)? {
        Ok(true)
    } else {
        registry.dispatch(envelope, sink)
    }
}
