//! ftrace plugin domain decoding.

use anyhow::Result;

use crate::{
    decode::profiler::{ProfilerPayloadRoute, ProfilerPluginRoute, roots::FTRACE_ROOT_MESSAGE},
    domains::ftrace::{FtraceCaptureRecord, FtraceEventRecord, FtraceRecord},
    formats::hitrace::profiler::{PluginEnvelope, decode_payload},
    proto::TracePluginResult,
    record::{DecodedPayload, TraceRecord, TraceRecordSink},
};

const FTRACE_PLUGIN_NAME: &str = "ftrace-plugin";

pub(super) const FTRACE_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: FTRACE_PLUGIN_NAME,
    config: None,
    data: ProfilerPayloadRoute {
        root_message: FTRACE_ROOT_MESSAGE,
        emit: emit_ftrace_payload,
    },
};

fn emit_ftrace_payload(
    root_message: &'static str,
    envelope: &PluginEnvelope<'_>,
    sink: &mut dyn TraceRecordSink,
) -> Result<()> {
    let result: TracePluginResult = decode_payload(envelope)?;
    if sink.accepts_decoded_payloads() {
        let payload = DecodedPayload::from_typed_message(root_message, &result)?;
        sink.push(TraceRecord::DecodedPayload(Box::new(payload)))?;
    }
    if !sink.accepts_source_records() {
        return Ok(());
    }

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
