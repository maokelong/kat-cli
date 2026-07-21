use anyhow::Result;

use crate::{
    formats::hitrace::profiler::{
        PluginDecoder, PluginDecoderSpec, PluginEnvelope, decode_payload,
    },
    proto::TracePluginResult,
    record::{TraceRecord, TraceRecordSink},
};

use super::{FtraceCaptureRecord, FtraceEventRecord, FtraceRecord};

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
    let result: TracePluginResult = decode_payload(envelope)?;

    for stats in result.ftrace_cpu_stats {
        sink.push(TraceRecord::FtraceCapture(FtraceCaptureRecord::CpuStats(
            stats,
        )))?;
    }

    if !result.clocks_detail.is_empty() {
        sink.push(TraceRecord::FtraceCapture(
            FtraceCaptureRecord::ClockSnapshot(result.clocks_detail),
        ))?;
    }

    for detail in result.ftrace_cpu_detail {
        sink.push(TraceRecord::FtraceCapture(FtraceCaptureRecord::CpuDetail {
            cpu: detail.cpu,
            overwrite: detail.overwrite,
        }))?;
        for event in detail.event {
            sink.push(TraceRecord::Ftrace(Box::new(FtraceRecord::Event(
                Box::new(FtraceEventRecord::new(detail.cpu, event)),
            ))))?;
        }
    }

    Ok(())
}
