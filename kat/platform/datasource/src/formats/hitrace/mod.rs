//! `.htrace` profiler container format adapter.

mod file;
pub(crate) mod profiler;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use anyhow::{Result, bail};
use log::debug;

use crate::{
    domains::{
        ftrace::FTRACE_PLUGIN_DECODER,
        native_hook::{HOOK_DAEMON_PLUGIN_DECODER, NATIVE_HOOK_PLUGIN_DECODER},
    },
    mmap::with_mapped_file,
    record::TraceRecordSink,
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
    pub(crate) unsupported_content: Vec<UnsupportedHitraceContent>,
    pub(crate) clock_domains: BTreeMap<String, String>,
    pub(crate) clock_snapshots: Vec<ClockSnapshot>,
}

#[derive(Clone, Debug)]
pub(crate) struct ClockSnapshot {
    pub(crate) snapshot_id: u64,
    pub(crate) clock_domain: String,
    pub(crate) clock_value: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct UnsupportedHitraceContent {
    pub(crate) kind: &'static str,
    pub(crate) value: String,
    pub(crate) byte_offset: usize,
}

pub(crate) fn decode_file(path: &Path, sink: &mut impl TraceRecordSink) -> Result<()> {
    debug!("decoding hitrace format from {}", path.display());
    with_mapped_file(path, |bytes| decode_bytes(bytes, sink))
}

pub(crate) fn decode_file_with_report(
    path: &Path,
    sink: &mut impl TraceRecordSink,
) -> Result<HitraceDecodeReport> {
    debug!("decoding hitrace format from {}", path.display());
    let mut report = HitraceDecodeReport::default();
    with_mapped_file(path, |bytes| {
        decode_bytes_with_report(bytes, sink, &mut report)
    })?;
    Ok(report)
}

fn decode_bytes(bytes: &[u8], sink: &mut impl TraceRecordSink) -> Result<()> {
    if !has_profiler_header(bytes) {
        bail!("invalid hitrace file: missing OHOSPROF header");
    }

    decode_sections(bytes, sink)
}

fn decode_bytes_with_report(
    bytes: &[u8],
    sink: &mut impl TraceRecordSink,
    report: &mut HitraceDecodeReport,
) -> Result<()> {
    if !has_profiler_header(bytes) {
        bail!("invalid hitrace file: missing OHOSPROF header");
    }

    decode_sections_with_report(bytes, sink, report)
}

fn decode_sections(bytes: &[u8], sink: &mut impl TraceRecordSink) -> Result<()> {
    decode_sections_inner(bytes, sink, None)
}

fn decode_sections_with_report(
    bytes: &[u8],
    sink: &mut impl TraceRecordSink,
    report: &mut HitraceDecodeReport,
) -> Result<()> {
    decode_sections_inner(bytes, sink, Some(report))
}

fn decode_sections_inner(
    bytes: &[u8],
    sink: &mut impl TraceRecordSink,
    mut report: Option<&mut HitraceDecodeReport>,
) -> Result<()> {
    let mut offset = 0usize;
    let decoder_specs = [
        FTRACE_PLUGIN_DECODER,
        NATIVE_HOOK_PLUGIN_DECODER,
        HOOK_DAEMON_PLUGIN_DECODER,
    ];
    let mut plugin_registry = PluginPayloadRegistry::new(&decoder_specs);

    while offset < bytes.len() {
        let section = read_profiler_section(bytes, offset)?;
        offset = section.end;

        if section.start == 0
            && let Some(report) = report.as_deref_mut()
        {
            add_header_clock_facts(bytes, report)?;
        }

        if section.header.data_type != HIPROFILER_PROTOBUF_BIN {
            if let Some(report) = report.as_deref_mut() {
                report
                    .unsupported_section_types
                    .insert(section.header.data_type);
                report.unsupported_content.push(UnsupportedHitraceContent {
                    kind: "section_type",
                    value: section.header.data_type.to_string(),
                    byte_offset: section.start,
                });
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
                        report.unsupported_plugins.insert(plugin.to_owned());
                        report.unsupported_content.push(UnsupportedHitraceContent {
                            kind: "plugin",
                            value: plugin.to_owned(),
                            byte_offset: section_body_start + frame_offset,
                        });
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

fn add_header_clock_facts(bytes: &[u8], report: &mut HitraceDecodeReport) -> Result<()> {
    const HEADER_CLOCKS: [(&str, &str, usize); 6] = [
        ("boottime", "boottime", 60),
        ("realtime", "realtime", 68),
        ("realtime_coarse", "realtime_coarse", 76),
        ("monotonic", "monotonic", 84),
        ("monotonic_coarse", "monotonic_coarse", 92),
        ("monotonic_raw", "monotonic_raw", 100),
    ];

    for (domain, clock_type, offset) in HEADER_CLOCKS {
        let end = offset + std::mem::size_of::<u64>();
        let value = u64::from_le_bytes(bytes[offset..end].try_into()?);
        report
            .clock_domains
            .insert(domain.to_owned(), clock_type.to_owned());
        report.clock_snapshots.push(ClockSnapshot {
            snapshot_id: 0,
            clock_domain: domain.to_owned(),
            clock_value: value,
        });
    }
    Ok(())
}
