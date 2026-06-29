//! `.htrace` profiler container format adapter.

mod file;
pub(crate) mod profiler;

use std::path::Path;

use anyhow::{Result, bail};
use log::debug;

use crate::{
    domains::{
        fixed_result::FIXED_RESULT_PLUGIN_DECODERS,
        ftrace::FTRACE_PLUGIN_DECODER,
        native_hook::{HOOK_DAEMON_PLUGIN_DECODER, NATIVE_HOOK_PLUGIN_DECODER},
    },
    mmap::with_mapped_file,
    record::TraceRecordSink,
};

use file::{HIPROFILER_PROTOBUF_BIN, has_profiler_header, read_profiler_section};
use profiler::{PluginPayloadRegistry, decode_plugin_section_body};

pub(crate) fn decode_file(path: &Path, sink: &mut impl TraceRecordSink) -> Result<()> {
    debug!("decoding hitrace format from {}", path.display());
    with_mapped_file(path, |bytes| decode_bytes(bytes, sink))
}

fn decode_bytes(bytes: &[u8], sink: &mut impl TraceRecordSink) -> Result<()> {
    if !has_profiler_header(bytes) {
        bail!("invalid hitrace file: missing OHOSPROF header");
    }

    decode_sections(bytes, sink)
}

fn decode_sections(bytes: &[u8], sink: &mut impl TraceRecordSink) -> Result<()> {
    let mut offset = 0usize;
    let mut decoder_specs = vec![
        FTRACE_PLUGIN_DECODER,
        NATIVE_HOOK_PLUGIN_DECODER,
        HOOK_DAEMON_PLUGIN_DECODER,
    ];
    decoder_specs.extend_from_slice(FIXED_RESULT_PLUGIN_DECODERS);
    let mut plugin_registry = PluginPayloadRegistry::new(&decoder_specs);

    while offset < bytes.len() {
        let section = read_profiler_section(bytes, offset)?;
        offset = section.end;

        if section.header.data_type != HIPROFILER_PROTOBUF_BIN {
            debug!(
                "skip unsupported profiler section data_type={} section_len={}",
                section.header.data_type, section.header.length
            );
            continue;
        }

        decode_plugin_section_body(
            section.body(bytes),
            section.start,
            &mut plugin_registry,
            sink,
        )?;
    }

    plugin_registry.finish(sink)
}
