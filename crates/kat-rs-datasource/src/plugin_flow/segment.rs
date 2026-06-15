use anyhow::{Result, bail};
use prost::Message;

const SEGMENT_LENGTH_SIZE: usize = 4;

pub(crate) fn for_each_len_prefixed_message<T, F>(bytes: &[u8], mut visitor: F) -> Result<()>
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
        let message = T::decode(segment).map_err(|source| {
            anyhow::anyhow!(
                "failed to decode length-prefixed protobuf message at byte {offset}: {source}"
            )
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
