// `.htrace` 适配器只负责容器校验、section 遍历和插件流转入口调用。

mod file;

use std::path::Path;

use anyhow::{Result, bail};
use log::debug;

use crate::{
    mmap::with_mapped_file,
    plugin_flow::{PluginPayloadRegistry, decode_plugin_section_body},
    record::TraceRecordSink,
};

use file::{HIPROFILER_PROTOBUF_BIN, has_profiler_header, read_profiler_section};

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
    let mut plugin_registry = PluginPayloadRegistry::default();

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
