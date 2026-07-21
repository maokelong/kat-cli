use anyhow::Result;

use crate::{
    formats::hitrace::profiler::{
        PluginDecoder, PluginDecoderSpec, PluginEnvelope, decode_payload,
    },
    proto::{BatchNativeHookData, NativeHookConfig},
    record::{TraceRecord, TraceRecordSink},
};

use super::{NativeHookEventContext, NativeHookRecord, native_hook_record_from_event};

pub(crate) const NATIVE_HOOK_PLUGIN_NAME: &str = "nativehook";
pub(crate) const HOOK_DAEMON_PLUGIN_NAME: &str = "hookdaemon";
pub(crate) const NATIVE_HOOK_PLUGIN_DECODER: PluginDecoderSpec =
    PluginDecoderSpec::new(new_native_hook_plugin_decoder);
pub(crate) const HOOK_DAEMON_PLUGIN_DECODER: PluginDecoderSpec =
    PluginDecoderSpec::new(new_hook_daemon_plugin_decoder);

fn new_native_hook_plugin_decoder() -> Box<dyn PluginDecoder> {
    Box::new(NativeHookPluginDecoder {
        plugin_name: NATIVE_HOOK_PLUGIN_NAME,
    })
}

fn new_hook_daemon_plugin_decoder() -> Box<dyn PluginDecoder> {
    Box::new(NativeHookPluginDecoder {
        plugin_name: HOOK_DAEMON_PLUGIN_NAME,
    })
}

struct NativeHookPluginDecoder {
    plugin_name: &'static str,
}

impl PluginDecoder for NativeHookPluginDecoder {
    fn plugin_name(&self) -> &'static str {
        self.plugin_name
    }

    fn configure(
        &mut self,
        envelope: &PluginEnvelope<'_>,
        sink: &mut dyn TraceRecordSink,
    ) -> Result<()> {
        let config = decode_native_hook_config(envelope)?;
        push_native_hook_record(sink, NativeHookRecord::Config(Box::new(config)))?;
        Ok(())
    }

    fn decode_data(
        &mut self,
        envelope: &PluginEnvelope<'_>,
        sink: &mut dyn TraceRecordSink,
    ) -> Result<()> {
        decode_native_hook_payload(envelope, sink)
    }
}

fn decode_native_hook_config(envelope: &PluginEnvelope<'_>) -> Result<NativeHookConfig> {
    decode_payload(envelope)
}

fn decode_native_hook_payload(
    envelope: &PluginEnvelope<'_>,
    sink: &mut dyn TraceRecordSink,
) -> Result<()> {
    let batch: BatchNativeHookData = decode_payload(envelope)?;

    for data in batch.events {
        let context = NativeHookEventContext::new(data.tv_sec, data.tv_nsec);

        if let Some(record) = native_hook_record_from_event(context, data.event) {
            push_native_hook_record(sink, record)?;
        }
    }

    Ok(())
}

fn push_native_hook_record(sink: &mut dyn TraceRecordSink, record: NativeHookRecord) -> Result<()> {
    sink.push(TraceRecord::NativeHook(Box::new(record)))
}
