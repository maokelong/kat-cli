use anyhow::{Context, Result};

use crate::{
    catalog::{TraceRecord, TraceRecordSink},
    domains::ftrace::{FTRACE_PLUGIN_NAME, decode_plugin_payload},
    proto::ProfilerPluginData,
};

use super::{file::ProfilerSection, segment::for_each_len_prefixed_message};

pub(crate) fn decode_profiler_section(
    section: ProfilerSection,
    bytes: &[u8],
    sink: &mut impl TraceRecordSink,
) -> Result<()> {
    for_each_len_prefixed_message::<ProfilerPluginData, _>(section.body(bytes), |message| {
        dispatch_profiler_message(&message, section.start, sink)?;
        sink.push(TraceRecord::ProfilerPluginData(message))
            .with_context(|| {
                format!(
                    "failed to append profiler section at byte {} to trace sink",
                    section.start
                )
            })?;
        Ok(())
    })
    .with_context(|| format!("failed to parse profiler section at byte {}", section.start))
}

fn dispatch_profiler_message(
    message: &ProfilerPluginData,
    section_start: usize,
    sink: &mut impl TraceRecordSink,
) -> Result<()> {
    if message.name != FTRACE_PLUGIN_NAME {
        return Ok(());
    }

    decode_plugin_payload(message.data.as_slice(), section_start, sink)
}
