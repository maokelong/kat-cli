//! Parses hitrace files into Arrow batches backed by profiler plugin segments.

mod table_builder;

use std::path::Path;

use anyhow::{Context, Result, bail};
use arrow_array::RecordBatch;
use log::debug;
use prost::Message;

use crate::{
    mmap::with_mapped_file,
    proto::{ProfilerPluginData, TracePluginResult},
    sched_table_builders::SchedDirectTableBuilders,
};

pub(crate) use table_builder::TableBuilder;

pub(crate) const HITRACE_TABLE: &str = "profiler_plugin_data";

const FTRACE_PLUGIN_NAME: &str = "ftrace-plugin";
const PROFILER_HEADER_SIZE: usize = 1024;
const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
const HIPROFILER_PROTOBUF_BIN: u32 = 0;
const SEGMENT_LENGTH_SIZE: usize = 4;

pub(crate) struct HitraceTable {
    pub(crate) name: &'static str,
    pub(crate) batches: Vec<RecordBatch>,
}

pub(crate) struct HitraceTables {
    pub(crate) profiler_plugin_data: Vec<RecordBatch>,
    pub(crate) tables: Vec<HitraceTable>,
}

pub(crate) fn load_hitrace_tables(path: &Path) -> Result<HitraceTables> {
    debug!("building hitrace datasource from {}", path.display());

    let tables = with_mapped_file(path, parse_hitrace_bytes)?;

    debug!(
        "built {} profiler batches and {} derived/event tables",
        tables.profiler_plugin_data.len(),
        tables.tables.len()
    );
    Ok(tables)
}

fn parse_hitrace_bytes(bytes: &[u8]) -> Result<HitraceTables> {
    if !has_profiler_header(bytes) {
        bail!("invalid hitrace file: missing OHOSPROF header");
    }

    parse_hitrace_sections(bytes)
}

fn has_profiler_header(bytes: &[u8]) -> bool {
    bytes.len() >= PROFILER_HEADER_SIZE
        && read_u64_le(bytes, 0)
            .map(|magic| magic == PROFILER_HEADER_MAGIC)
            .unwrap_or(false)
}

fn parse_hitrace_sections(bytes: &[u8]) -> Result<HitraceTables> {
    let mut offset = 0usize;
    let mut profiler_table = TableBuilder::<ProfilerPluginData>::new(HITRACE_TABLE)?;
    let mut profiler_section_count = 0usize;
    let mut sched_tables = SchedDirectTableBuilders::new()?;

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

        profiler_section_count += 1;
        for_each_len_prefixed_message::<ProfilerPluginData, _>(section.body(bytes), |message| {
            decode_sched_message(&message, section.start, &mut sched_tables)?;
            profiler_table.push(message).with_context(|| {
                format!(
                    "failed to append profiler section at byte {} to Arrow builder",
                    section.start
                )
            })?;
            Ok(())
        })
        .with_context(|| format!("failed to parse profiler section at byte {}", section.start))?;
    }

    let tables = sched_tables.into_tables()?;
    let profiler_plugin_data = if profiler_section_count == 0 {
        Vec::new()
    } else {
        profiler_table.into_table()?.batches
    };

    Ok(HitraceTables {
        profiler_plugin_data,
        tables,
    })
}

fn decode_sched_message(
    message: &ProfilerPluginData,
    section_start: usize,
    sched_tables: &mut SchedDirectTableBuilders,
) -> Result<()> {
    if message.name != FTRACE_PLUGIN_NAME {
        return Ok(());
    }

    let result = TracePluginResult::decode(message.data.as_slice()).with_context(|| {
        format!("failed to decode ftrace payload in profiler section at byte {section_start}")
    })?;
    for detail in result.ftrace_cpu_detail {
        for event in detail.event {
            sched_tables.push_event(detail.cpu, event)?;
        }
    }

    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct ProfilerSection {
    start: usize,
    end: usize,
    len: usize,
    data_type: u32,
}

impl ProfilerSection {
    fn body<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        &bytes[self.start + PROFILER_HEADER_SIZE..self.end]
    }
}

fn read_profiler_section(bytes: &[u8], offset: usize) -> Result<ProfilerSection> {
    ensure_available(bytes, offset, PROFILER_HEADER_SIZE, "profiler header")?;

    let magic = read_u64_le(bytes, offset)?;
    if magic != PROFILER_HEADER_MAGIC {
        bail!("invalid profiler header magic at byte {offset}: 0x{magic:x}");
    }

    let len = usize::try_from(read_u64_le(bytes, offset + 8)?)
        .with_context(|| format!("invalid profiler section length at byte {offset}"))?;
    let data_type = read_u32_le(bytes, offset + 56)?;
    let Some(end) = offset.checked_add(len) else {
        bail!("invalid profiler section length {len} at byte {offset}");
    };
    if len < PROFILER_HEADER_SIZE || end > bytes.len() {
        bail!("invalid profiler section length {len} at byte {offset}");
    }

    Ok(ProfilerSection {
        start: offset,
        end,
        len,
        data_type,
    })
}

fn for_each_len_prefixed_message<T, F>(bytes: &[u8], mut visitor: F) -> Result<()>
where
    T: Message + Default,
    F: FnMut(T) -> Result<()>,
{
    let mut offset = 0usize;

    while offset < bytes.len() {
        ensure_available(bytes, offset, SEGMENT_LENGTH_SIZE, "segment length")?;
        let len = read_u32_le(bytes, offset)? as usize;
        offset += SEGMENT_LENGTH_SIZE;
        ensure_available(bytes, offset, len, "profiler segment")?;

        let segment = &bytes[offset..offset + len];
        let message = T::decode(segment).with_context(|| {
            format!("failed to decode length-prefixed protobuf message at byte {offset}")
        })?;
        visitor(message)?;
        offset += len;
    }

    Ok(())
}

fn ensure_available(bytes: &[u8], offset: usize, len: usize, context: &str) -> Result<()> {
    if bytes.len().saturating_sub(offset) < len {
        bail!("truncated {context} at byte {offset}");
    }

    Ok(())
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32> {
    ensure_available(bytes, offset, 4, "u32")?;
    Ok(u32::from_le_bytes(bytes[offset..offset + 4].try_into()?))
}

fn read_u64_le(bytes: &[u8], offset: usize) -> Result<u64> {
    ensure_available(bytes, offset, 8, "u64")?;
    Ok(u64::from_le_bytes(bytes[offset..offset + 8].try_into()?))
}
