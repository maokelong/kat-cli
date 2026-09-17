//! `.htrace` profiler container format adapter.

pub(crate) mod file;
pub(crate) mod profiler;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use anyhow::{Result, bail};
use log::debug;

use crate::mmap::with_mapped_file;

use file::{
    HIPROFILER_PROTOBUF_BIN, PROFILER_HEADER_SIZE, has_profiler_header, read_profiler_section,
};
use profiler::{PluginPayloadClaimant, decode_section};

#[derive(Debug, Default)]
pub(crate) struct HitraceDecodeReport {
    pub(crate) unsupported_plugins: BTreeSet<String>,
    pub(crate) unsupported_section_types: BTreeSet<u32>,
    pub(crate) clock_domains: BTreeMap<String, String>,
    pub(crate) clock_snapshots: Vec<ClockSnapshot>,
}

#[derive(Clone, Debug)]
pub(crate) struct ClockSnapshot {
    pub(crate) snapshot_id: u64,
    pub(crate) clock_domain: String,
    pub(crate) clock_value: u64,
}

pub(crate) fn decode_file(
    path: &Path,
    claimant: &mut impl PluginPayloadClaimant,
) -> Result<HitraceDecodeReport> {
    debug!("decoding hitrace format from {}", path.display());
    with_mapped_file(path, |bytes| decode_bytes(bytes, claimant))
}

fn decode_bytes(
    bytes: &[u8],
    claimant: &mut impl PluginPayloadClaimant,
) -> Result<HitraceDecodeReport> {
    if !has_profiler_header(bytes) {
        bail!("invalid hitrace file: missing OHOSPROF header");
    }

    let mut report = HitraceDecodeReport::default();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let section = read_profiler_section(bytes, offset)?;
        offset = section.end;

        if section.start == 0 {
            add_header_clock_facts(bytes, &mut report)?;
        }

        if section.header.data_type != HIPROFILER_PROTOBUF_BIN {
            report
                .unsupported_section_types
                .insert(section.header.data_type);
            debug!(
                "skip unsupported profiler section data_type={} section_len={}",
                section.header.data_type, section.header.length
            );
            continue;
        }

        let section_body_start = section.start + PROFILER_HEADER_SIZE;
        decode_section(
            section.body(bytes),
            section_body_start,
            claimant,
            |message, known, _frame_offset| {
                if !known {
                    report.unsupported_plugins.insert(
                        message
                            .name
                            .strip_suffix("_config")
                            .unwrap_or(message.name.as_str())
                            .to_owned(),
                    );
                }
                Ok(())
            },
        )?;
    }

    Ok(report)
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
