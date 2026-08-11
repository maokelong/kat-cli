pub(crate) mod fixed_result;
pub(crate) mod ftrace;
pub(crate) mod native_hook;
pub(crate) mod roots;

use anyhow::Result;
use prost::Message;
use serde::Serialize;

use crate::{
    formats::hitrace::profiler::{PluginDecoder, PluginEnvelope, decode_payload},
    record::{DecodedPayload, TraceRecord, TraceRecordSink},
};

#[derive(Clone, Copy)]
pub(crate) struct ProfilerPayloadRoute {
    pub(crate) root_message: &'static str,
    pub(crate) emit: ProfilerPayloadEmitFn,
}

#[derive(Clone, Copy)]
pub(crate) struct ProfilerPluginRoute {
    pub(crate) plugin_name: &'static str,
    pub(crate) config: Option<ProfilerPayloadRoute>,
    pub(crate) data: ProfilerPayloadRoute,
}

pub(crate) type ProfilerPayloadEmitFn =
    fn(&'static str, &PluginEnvelope<'_>, &mut dyn TraceRecordSink) -> Result<()>;

struct ProfilerPluginDecoder {
    route: &'static ProfilerPluginRoute,
}

pub(crate) fn new_profiler_plugin_decoder(
    route: &'static ProfilerPluginRoute,
) -> Box<dyn PluginDecoder> {
    Box::new(ProfilerPluginDecoder { route })
}

pub(crate) const PROFILER_PLUGIN_ROUTES: &[ProfilerPluginRoute] = &[
    fixed_result::CPU_ROUTE,
    fixed_result::MEMORY_ROUTE,
    fixed_result::PROCESS_ROUTE,
    fixed_result::DISKIO_ROUTE,
    fixed_result::NETWORK_ROUTE,
    fixed_result::GPU_ROUTE,
    ftrace::FTRACE_ROUTE,
    native_hook::NATIVE_HOOK_ROUTE,
    native_hook::HOOK_DAEMON_ROUTE,
];

pub(crate) fn source_fact_plugin_decoders() -> Vec<Box<dyn PluginDecoder>> {
    [
        &ftrace::FTRACE_ROUTE,
        &native_hook::NATIVE_HOOK_ROUTE,
        &native_hook::HOOK_DAEMON_ROUTE,
    ]
    .into_iter()
    .map(new_profiler_plugin_decoder)
    .collect()
}

pub(crate) fn relational_plugin_decoders() -> Vec<Box<dyn PluginDecoder>> {
    debug_assert!(PROFILER_PLUGIN_ROUTES.iter().all(|route| {
        route
            .config
            .into_iter()
            .chain(std::iter::once(route.data))
            .all(|payload| roots::RELATIONAL_ROOT_MESSAGES.contains(&payload.root_message))
    }));
    debug_assert!(roots::RELATIONAL_ROOT_MESSAGES.iter().all(|root| {
        PROFILER_PLUGIN_ROUTES.iter().any(|route| {
            route
                .config
                .into_iter()
                .chain(std::iter::once(route.data))
                .any(|payload| payload.root_message == *root)
        })
    }));
    PROFILER_PLUGIN_ROUTES
        .iter()
        .map(new_profiler_plugin_decoder)
        .collect()
}

impl PluginDecoder for ProfilerPluginDecoder {
    fn plugin_name(&self) -> &'static str {
        self.route.plugin_name
    }

    fn configure(
        &mut self,
        envelope: &PluginEnvelope<'_>,
        sink: &mut dyn TraceRecordSink,
    ) -> Result<()> {
        let Some(config) = self.route.config else {
            return Ok(());
        };
        (config.emit)(config.root_message, envelope, sink)
    }

    fn decode_data(
        &mut self,
        envelope: &PluginEnvelope<'_>,
        sink: &mut dyn TraceRecordSink,
    ) -> Result<()> {
        (self.route.data.emit)(self.route.data.root_message, envelope, sink)
    }
}

pub(crate) fn emit_typed_payload<T>(
    root_message: &'static str,
    envelope: &PluginEnvelope<'_>,
    sink: &mut dyn TraceRecordSink,
) -> Result<()>
where
    T: Message + Default + Serialize,
{
    let message: T = decode_payload(envelope)?;
    if !sink.accepts_decoded_payloads() {
        return Ok(());
    }
    let payload = DecodedPayload::from_typed_message(root_message, &message)?;
    sink.push(TraceRecord::DecodedPayload(Box::new(payload)))
}
