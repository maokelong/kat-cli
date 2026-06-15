use anyhow::{Result, bail};

pub(crate) const PROFILER_HEADER_SIZE: usize = 1024;
const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
pub(crate) const HIPROFILER_PROTOBUF_BIN: u32 = 0;
const HEADER_LENGTH_OFFSET: usize = 8;
const HEADER_DATA_TYPE_OFFSET: usize = 56;

pub(crate) fn has_profiler_header(bytes: &[u8]) -> bool {
    TraceFileHeader::read_at(bytes, 0)
        .map(|header| header.magic == PROFILER_HEADER_MAGIC)
        .unwrap_or(false)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TraceFileHeader {
    pub(crate) magic: u64,
    pub(crate) length: usize,
    pub(crate) data_type: u32,
}

impl TraceFileHeader {
    fn read_at(bytes: &[u8], offset: usize) -> Result<Self> {
        ensure_available(bytes, offset, PROFILER_HEADER_SIZE, "profiler header")?;

        let magic = read_u64_le(bytes, offset)?;
        let length = usize::try_from(read_u64_le(bytes, offset + HEADER_LENGTH_OFFSET)?)?;
        let data_type = read_u32_le(bytes, offset + HEADER_DATA_TYPE_OFFSET)?;

        Ok(Self {
            magic,
            length,
            data_type,
        })
    }

    fn validate_at(&self, bytes: &[u8], offset: usize) -> Result<usize> {
        if self.magic != PROFILER_HEADER_MAGIC {
            bail!(
                "invalid profiler header magic at byte {offset}: 0x{:x}",
                self.magic
            );
        }

        let Some(end) = offset.checked_add(self.length) else {
            bail!(
                "invalid profiler section length {} at byte {offset}",
                self.length
            );
        };
        if self.length < PROFILER_HEADER_SIZE || end > bytes.len() {
            bail!(
                "invalid profiler section length {} at byte {offset}",
                self.length
            );
        }

        Ok(end)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProfilerSection {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) header: TraceFileHeader,
}

impl ProfilerSection {
    pub(crate) fn body<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        &bytes[self.start + PROFILER_HEADER_SIZE..self.end]
    }
}

pub(crate) fn read_profiler_section(bytes: &[u8], offset: usize) -> Result<ProfilerSection> {
    let header = TraceFileHeader::read_at(bytes, offset)?;
    let end = header.validate_at(bytes, offset)?;

    Ok(ProfilerSection {
        start: offset,
        end,
        header,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_profiler_section_exposes_trace_file_header_fields() {
        let section_len = PROFILER_HEADER_SIZE + 12;
        let mut bytes = vec![0; section_len];
        bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
        bytes[HEADER_LENGTH_OFFSET..HEADER_LENGTH_OFFSET + 8]
            .copy_from_slice(&(section_len as u64).to_le_bytes());
        bytes[HEADER_DATA_TYPE_OFFSET..HEADER_DATA_TYPE_OFFSET + 4]
            .copy_from_slice(&HIPROFILER_PROTOBUF_BIN.to_le_bytes());

        let section = read_profiler_section(&bytes, 0).expect("section can be read");

        assert_eq!(section.start, 0);
        assert_eq!(section.body(&bytes).len(), 12);
        assert_eq!(section.end, section_len);
        assert_eq!(section.header.magic, PROFILER_HEADER_MAGIC);
        assert_eq!(section.header.length, section_len);
        assert_eq!(section.header.data_type, HIPROFILER_PROTOBUF_BIN);
    }
}
