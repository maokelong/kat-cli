// ftrace 插件 decoder 按 plugin_flow 生命周期接入，只处理 data envelope。
use anyhow::{Context, Result};
use prost::Message;

use crate::{
    plugin_flow::{PluginDecoder, PluginDecoderSpec, PluginEnvelope},
    proto::TracePluginResult,
    record::{TraceRecord, TraceRecordSink},
};

use super::FtraceEventRecord;

pub(crate) const FTRACE_PLUGIN_NAME: &str = "ftrace-plugin";
pub(crate) const FTRACE_PLUGIN_DECODER: PluginDecoderSpec =
    PluginDecoderSpec::new(new_ftrace_plugin_decoder);

fn new_ftrace_plugin_decoder() -> Box<dyn PluginDecoder> {
    Box::new(FtracePluginDecoder)
}

struct FtracePluginDecoder;

impl PluginDecoder for FtracePluginDecoder {
    fn plugin_name(&self) -> &'static str {
        FTRACE_PLUGIN_NAME
    }

    fn decode_data(
        &mut self,
        envelope: &PluginEnvelope<'_>,
        sink: &mut dyn TraceRecordSink,
    ) -> Result<()> {
        decode_plugin_payload(envelope, sink)
    }
}

fn decode_plugin_payload(
    envelope: &PluginEnvelope<'_>,
    sink: &mut dyn TraceRecordSink,
) -> Result<()> {
    let result = TracePluginResult::decode(envelope.payload).with_context(|| {
        format!(
            "failed to decode {} payload in profiler section at byte {} version={} sample_interval={}",
            envelope.envelope_name,
            envelope.section_start,
            envelope.version,
            envelope.sample_interval
        )
    })?;

    for detail in result.ftrace_cpu_detail {
        for event in detail.event {
            sink.push(TraceRecord::FtraceEvent(Box::new(FtraceEventRecord::new(
                detail.cpu, event,
            ))))?;
        }
    }

    Ok(())
}
