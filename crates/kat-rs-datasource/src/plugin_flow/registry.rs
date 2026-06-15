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

    #[cfg(test)]
    pub(crate) fn from_decoders(decoders: Vec<Box<dyn PluginDecoder>>) -> Self {
        Self { decoders }
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::{cell::RefCell, rc::Rc};

    use crate::{proto::ProfilerPluginData, record::TraceRecord};

    #[derive(Default)]
    struct RecordingSink {
        records: Vec<TraceRecord>,
    }

    impl TraceRecordSink for RecordingSink {
        fn push(&mut self, record: TraceRecord) -> Result<()> {
            self.records.push(record);
            Ok(())
        }
    }

    struct RecordingDecoder {
        events: Rc<RefCell<Vec<String>>>,
    }

    impl RecordingDecoder {
        fn new(events: Rc<RefCell<Vec<String>>>) -> Self {
            Self { events }
        }
    }

    impl PluginDecoder for RecordingDecoder {
        fn plugin_name(&self) -> &'static str {
            "demo-plugin"
        }

        fn configure(&mut self, envelope: &PluginEnvelope<'_>) -> Result<()> {
            self.events
                .borrow_mut()
                .push(format!("configure:{}", envelope.envelope_name));
            Ok(())
        }

        fn decode_data(
            &mut self,
            envelope: &PluginEnvelope<'_>,
            _sink: &mut dyn TraceRecordSink,
        ) -> Result<()> {
            self.events
                .borrow_mut()
                .push(format!("decode_data:{}", envelope.envelope_name));
            Ok(())
        }

        fn finish(&mut self, _sink: &mut dyn TraceRecordSink) -> Result<()> {
            self.events.borrow_mut().push("finish".to_string());
            Ok(())
        }
    }

    fn plugin_message(name: &str) -> ProfilerPluginData {
        ProfilerPluginData {
            name: name.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn registry_dispatches_config_data_and_finish_to_matching_decoder() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut registry = PluginPayloadRegistry::from_decoders(vec![Box::new(
            RecordingDecoder::new(Rc::clone(&events)),
        )]);
        let mut sink = RecordingSink::default();

        let config = plugin_message("demo-plugin_config");
        let config = PluginEnvelope::from_profiler_plugin_data(&config, 10);
        registry
            .dispatch(&config, &mut sink)
            .expect("config dispatch");

        let data = plugin_message("demo-plugin");
        let data = PluginEnvelope::from_profiler_plugin_data(&data, 20);
        registry.dispatch(&data, &mut sink).expect("data dispatch");

        let unknown = plugin_message("unknown-plugin");
        let unknown = PluginEnvelope::from_profiler_plugin_data(&unknown, 30);
        registry
            .dispatch(&unknown, &mut sink)
            .expect("unknown dispatch");

        registry.finish(&mut sink).expect("finish dispatch");

        assert_eq!(
            events.borrow().clone(),
            vec![
                "configure:demo-plugin_config".to_string(),
                "decode_data:demo-plugin".to_string(),
                "finish".to_string(),
            ]
        );
    }
}
