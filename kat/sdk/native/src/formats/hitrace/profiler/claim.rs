use anyhow::Result;

use super::PluginEnvelope;

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
