//! `.htrace` profiler container format adapter.

mod file;
mod profiler;
mod segment;

use std::path::Path;

use anyhow::{Result, bail};
use log::debug;

use crate::{catalog::TraceRecordSink, mmap::with_mapped_file};

use file::{HIPROFILER_PROTOBUF_BIN, has_profiler_header, read_profiler_section};
use profiler::decode_profiler_section;

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

    while offset < bytes.len() {
        let section = read_profiler_section(bytes, offset)?;
        offset = section.end;

        if section.data_type != HIPROFILER_PROTOBUF_BIN {
            debug!(
                "skip unsupported profiler section data_type={} section_len={}",
                section.data_type, section.len
            );
            continue;
        }

        decode_profiler_section(section, bytes, sink)?;
    }

    Ok(())
}
