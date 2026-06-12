use anyhow::{Result, bail};

pub(crate) const PROFILER_HEADER_SIZE: usize = 1024;
const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
pub(crate) const HIPROFILER_PROTOBUF_BIN: u32 = 0;

pub(crate) fn has_profiler_header(bytes: &[u8]) -> bool {
    bytes.len() >= PROFILER_HEADER_SIZE
        && read_u64_le(bytes, 0)
            .map(|magic| magic == PROFILER_HEADER_MAGIC)
            .unwrap_or(false)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProfilerSection {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) len: usize,
    pub(crate) data_type: u32,
}

impl ProfilerSection {
    pub(crate) fn body<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        &bytes[self.start + PROFILER_HEADER_SIZE..self.end]
    }
}

pub(crate) fn read_profiler_section(bytes: &[u8], offset: usize) -> Result<ProfilerSection> {
    ensure_available(bytes, offset, PROFILER_HEADER_SIZE, "profiler header")?;

    let magic = read_u64_le(bytes, offset)?;
    if magic != PROFILER_HEADER_MAGIC {
        bail!("invalid profiler header magic at byte {offset}: 0x{magic:x}");
    }

    let len = usize::try_from(read_u64_le(bytes, offset + 8)?)?;
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

pub(crate) fn ensure_available(
    bytes: &[u8],
    offset: usize,
    len: usize,
    context: &str,
) -> Result<()> {
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
