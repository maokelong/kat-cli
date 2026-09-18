use anyhow::{Result, bail};
use prost::Message;

use crate::proto::ProfilerPluginData;

const FRAME_LENGTH_SIZE: usize = 4;

pub(crate) fn for_each_profiler_envelope_frame<F>(bytes: &[u8], mut visitor: F) -> Result<()>
where
    F: FnMut(ProfilerPluginData, usize) -> Result<()>,
{
    let mut offset = 0usize;

    while offset < bytes.len() {
        ensure_available(
            bytes,
            offset,
            FRAME_LENGTH_SIZE,
            "profiler envelope frame length",
        )?;
        let len = read_u32_le(bytes, offset)? as usize;
        offset += FRAME_LENGTH_SIZE;
        ensure_available(bytes, offset, len, "profiler envelope frame")?;

        let frame = &bytes[offset..offset + len];
        let message = ProfilerPluginData::decode(frame).map_err(|source| {
            anyhow::anyhow!("failed to decode profiler envelope frame at byte {offset}: {source}")
        })?;
        visitor(message, offset)?;
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
