//! `.htrace` profiler container format adapter.

mod file;
pub(crate) mod profiler;

use std::{collections::BTreeSet, path::Path};

use anyhow::{Result, bail};
use log::debug;

use crate::{
    decode::profiler::profiler_plugin_decoders, mmap::with_mapped_file, record::TraceRecordSink,
};

use file::{
    HIPROFILER_PROTOBUF_BIN, PROFILER_HEADER_SIZE, has_profiler_header, read_profiler_section,
};
use profiler::{
    PluginPayloadRegistry, decode_plugin_section_body, decode_plugin_section_body_with_observer,
};

#[derive(Debug, Default)]
pub(crate) struct HitraceDecodeReport {
    pub(crate) unsupported_plugins: BTreeSet<String>,
    pub(crate) unsupported_section_types: BTreeSet<u32>,
}

#[derive(Debug)]
pub(crate) struct HitraceDecodeFailure {
    pub(crate) source: anyhow::Error,
}

#[derive(Clone, Debug)]
pub(crate) struct UnsupportedHitraceContent {
    pub(crate) kind: &'static str,
    pub(crate) value: String,
    pub(crate) byte_offset: usize,
}

type UnsupportedContentObserver<'a> = dyn FnMut(&UnsupportedHitraceContent) -> Result<()> + 'a;

pub(crate) fn decode_file_with_report(
    path: &Path,
    sink: &mut impl TraceRecordSink,
    observe_unsupported: &mut impl FnMut(&UnsupportedHitraceContent) -> Result<()>,
) -> std::result::Result<HitraceDecodeReport, HitraceDecodeFailure> {
    debug!("decoding hitrace format from {}", path.display());
    let mut report = HitraceDecodeReport::default();
    let result = with_mapped_file(path, |bytes| {
        decode_bytes_with_report(bytes, sink, &mut report, observe_unsupported)
    });
    match result {
        Ok(()) => Ok(report),
        Err(source) => Err(HitraceDecodeFailure { source }),
    }
}

fn decode_bytes_with_report(
    bytes: &[u8],
    sink: &mut impl TraceRecordSink,
    report: &mut HitraceDecodeReport,
    observe_unsupported: &mut impl FnMut(&UnsupportedHitraceContent) -> Result<()>,
) -> Result<()> {
    if !has_profiler_header(bytes) {
        bail!("invalid hitrace file: missing OHOSPROF header");
    }

    decode_sections_with_report(bytes, sink, report, observe_unsupported)
}

fn decode_sections_with_report(
    bytes: &[u8],
    sink: &mut impl TraceRecordSink,
    report: &mut HitraceDecodeReport,
    observe_unsupported: &mut impl FnMut(&UnsupportedHitraceContent) -> Result<()>,
) -> Result<()> {
    decode_sections_inner(bytes, sink, Some(report), Some(observe_unsupported))
}

fn decode_sections_inner(
    bytes: &[u8],
    sink: &mut impl TraceRecordSink,
    mut report: Option<&mut HitraceDecodeReport>,
    mut observe_unsupported: Option<&mut UnsupportedContentObserver<'_>>,
) -> Result<()> {
    let mut offset = 0usize;
    let mut plugin_registry = PluginPayloadRegistry::new(profiler_plugin_decoders());

    while offset < bytes.len() {
        let section = read_profiler_section(bytes, offset)?;
        offset = section.end;

        if section.header.data_type != HIPROFILER_PROTOBUF_BIN {
            let unsupported = UnsupportedHitraceContent {
                kind: "section_type",
                value: section.header.data_type.to_string(),
                byte_offset: section.start,
            };
            if let Some(observer) = observe_unsupported.as_deref_mut() {
                observer(&unsupported)?;
            }
            if let Some(report) = report.as_deref_mut() {
                report
                    .unsupported_section_types
                    .insert(section.header.data_type);
            }
            debug!(
                "skip unsupported profiler section data_type={} section_len={}",
                section.header.data_type, section.header.length
            );
            continue;
        }

        let section_body_start = section.start + PROFILER_HEADER_SIZE;
        if let Some(report) = report.as_deref_mut() {
            decode_plugin_section_body_with_observer(
                section.body(bytes),
                section_body_start,
                &mut plugin_registry,
                sink,
                |message, known, frame_offset| {
                    if !known {
                        let plugin = message
                            .name
                            .strip_suffix("_config")
                            .unwrap_or(message.name.as_str());
                        let unsupported = UnsupportedHitraceContent {
                            kind: "plugin",
                            value: plugin.to_owned(),
                            byte_offset: section_body_start + frame_offset,
                        };
                        if let Some(observer) = observe_unsupported.as_deref_mut() {
                            observer(&unsupported)?;
                        }
                        report.unsupported_plugins.insert(plugin.to_owned());
                    }
                    Ok(())
                },
            )?;
        } else {
            decode_plugin_section_body(
                section.body(bytes),
                section_body_start,
                &mut plugin_registry,
                sink,
            )?;
        }
    }

    plugin_registry.finish(sink)
}
