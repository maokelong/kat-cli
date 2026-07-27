//! native hook plugin domain decoding.

use anyhow::Result;

use crate::{
    decode::profiler::{ProfilerPayloadRoute, ProfilerPluginRoute},
    domains::native_hook::{
        NativeHookEventContext, NativeHookRecord, native_hook_record_from_event,
    },
    formats::hitrace::profiler::{PluginEnvelope, decode_payload},
    proto::{BatchNativeHookData, NativeHookConfig},
    record::{DecodedPayload, TraceRecord, TraceRecordSink},
};

const NATIVE_HOOK_PLUGIN_NAME: &str = "nativehook";
const HOOK_DAEMON_PLUGIN_NAME: &str = "hookdaemon";

pub(super) const NATIVE_HOOK_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: NATIVE_HOOK_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: "NativeHookConfig",
        emit: emit_native_hook_config,
    }),
    data: ProfilerPayloadRoute {
        root_message: "BatchNativeHookData",
        emit: emit_native_hook_data,
    },
};

pub(super) const HOOK_DAEMON_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: HOOK_DAEMON_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: "NativeHookConfig",
        emit: emit_native_hook_config,
    }),
    data: ProfilerPayloadRoute {
        root_message: "BatchNativeHookData",
        emit: emit_native_hook_data,
    },
};

fn emit_native_hook_config(
    plugin_name: &'static str,
    root_message: &'static str,
    envelope: &PluginEnvelope<'_>,
    sink: &mut dyn TraceRecordSink,
) -> Result<()> {
    let config: NativeHookConfig = decode_payload(envelope)?;
    if sink.accepts_decoded_payloads() {
        let payload = DecodedPayload::from_typed_message(plugin_name, root_message, &config)?;
        sink.push(TraceRecord::DecodedPayload(Box::new(payload)))?;
    }
    if !sink.accepts_source_records() {
        return Ok(());
    }
    sink.push(TraceRecord::NativeHook(Box::new(NativeHookRecord::Config(
        Box::new(config),
    ))))
}

fn emit_native_hook_data(
    plugin_name: &'static str,
    root_message: &'static str,
    envelope: &PluginEnvelope<'_>,
    sink: &mut dyn TraceRecordSink,
) -> Result<()> {
    let batch: BatchNativeHookData = decode_payload(envelope)?;
    if sink.accepts_decoded_payloads() {
        let payload = DecodedPayload::from_typed_message(plugin_name, root_message, &batch)?;
        sink.push(TraceRecord::DecodedPayload(Box::new(payload)))?;
    }
    if !sink.accepts_source_records() {
        return Ok(());
    }

    for data in batch.events {
        let context = NativeHookEventContext::new(data.tv_sec, data.tv_nsec);
        if let Some(record) = native_hook_record_from_event(context, data.event) {
            sink.push(TraceRecord::NativeHook(Box::new(record)))?;
        }
    }

    Ok(())
}
