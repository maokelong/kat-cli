use anyhow::Result;

use crate::{
    domains::ftrace::{FTRACE_PLUGIN_NAME, decode_plugin_payload},
    record::TraceRecordSink,
};

use super::{PluginEnvelope, PluginEnvelopeKind};

pub(crate) type DecodePluginPayload =
    for<'a> fn(&PluginEnvelope<'a>, &mut dyn TraceRecordSink) -> Result<()>;

pub(crate) struct PluginPayloadDecoder {
    plugin_name: &'static str,
    decode: DecodePluginPayload,
}

pub(crate) struct PluginPayloadRegistry {
    decoders: &'static [PluginPayloadDecoder],
}

impl PluginPayloadRegistry {
    pub(crate) const fn new(decoders: &'static [PluginPayloadDecoder]) -> Self {
        Self { decoders }
    }

    pub(crate) fn dispatch(
        &self,
        envelope: &PluginEnvelope<'_>,
        sink: &mut dyn TraceRecordSink,
    ) -> Result<()> {
        if envelope.kind != PluginEnvelopeKind::Data {
            return Ok(());
        }

        let Some(decoder) = self
            .decoders
            .iter()
            .find(|decoder| decoder.plugin_name == envelope.plugin_name)
        else {
            return Ok(());
        };

        (decoder.decode)(envelope, sink)
    }
}

static DECODERS: &[PluginPayloadDecoder] = &[PluginPayloadDecoder {
    plugin_name: FTRACE_PLUGIN_NAME,
    decode: decode_plugin_payload,
}];

pub(crate) static PLUGIN_PAYLOAD_REGISTRY: PluginPayloadRegistry =
    PluginPayloadRegistry::new(DECODERS);
