use anyhow::{Context, Result};
use prost::Message;

use crate::{
    proto::TracePluginResult,
    record::{TraceRecord, TraceRecordSink},
};

use super::FtraceEventRecord;

pub(crate) const FTRACE_PLUGIN_NAME: &str = "ftrace-plugin";

pub(crate) fn decode_plugin_payload(
    payload: &[u8],
    section_start: usize,
    sink: &mut impl TraceRecordSink,
) -> Result<()> {
    let result = TracePluginResult::decode(payload).with_context(|| {
        format!("failed to decode ftrace payload in profiler section at byte {section_start}")
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
