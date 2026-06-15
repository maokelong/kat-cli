// registry 负责按插件名匹配 decoder，并统一配置、数据和收尾生命周期。
use anyhow::Result;

use crate::{domains::ftrace::FTRACE_PLUGIN_DECODER, record::TraceRecordSink};

use super::{PluginEnvelope, PluginEnvelopeKind};

pub(crate) trait PluginDecoder {
    fn plugin_name(&self) -> &'static str;

    fn configure(&mut self, _envelope: &PluginEnvelope<'_>) -> Result<()> {
        Ok(())
    }

    fn decode_data(
        &mut self,
        envelope: &PluginEnvelope<'_>,
        sink: &mut dyn TraceRecordSink,
    ) -> Result<()>;

    fn finish(&mut self, _sink: &mut dyn TraceRecordSink) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PluginDecoderSpec {
    new_decoder: fn() -> Box<dyn PluginDecoder>,
}

impl PluginDecoderSpec {
    pub(crate) const fn new(new_decoder: fn() -> Box<dyn PluginDecoder>) -> Self {
        Self { new_decoder }
    }
}

pub(crate) struct PluginPayloadRegistry {
    decoders: Vec<Box<dyn PluginDecoder>>,
}

impl PluginPayloadRegistry {
    pub(crate) fn new(specs: &[PluginDecoderSpec]) -> Self {
        Self {
            decoders: specs
                .iter()
                .map(|spec| (spec.new_decoder)())
                .collect::<Vec<_>>(),
        }
    }

    pub(crate) fn dispatch(
        &mut self,
        envelope: &PluginEnvelope<'_>,
        sink: &mut dyn TraceRecordSink,
    ) -> Result<()> {
        let Some(decoder) = self
            .decoders
            .iter_mut()
            .find(|decoder| decoder.plugin_name() == envelope.plugin_name)
        else {
            return Ok(());
        };

        match envelope.kind {
            PluginEnvelopeKind::Config => decoder.configure(envelope),
            PluginEnvelopeKind::Data => decoder.decode_data(envelope, sink),
        }
    }

    pub(crate) fn finish(&mut self, sink: &mut dyn TraceRecordSink) -> Result<()> {
        for decoder in &mut self.decoders {
            decoder.finish(sink)?;
        }

        Ok(())
    }
}

static DECODERS: &[PluginDecoderSpec] = &[FTRACE_PLUGIN_DECODER];

impl Default for PluginPayloadRegistry {
    fn default() -> Self {
        Self::new(DECODERS)
    }
}
