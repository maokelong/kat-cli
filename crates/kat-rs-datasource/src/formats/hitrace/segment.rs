use anyhow::{Context, Result};
use prost::Message;

use super::file::ensure_available;

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
        let message = T::decode(segment).with_context(|| {
            format!("failed to decode length-prefixed protobuf message at byte {offset}")
        })?;
        visitor(message)?;
        offset += len;
    }

    Ok(())
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32> {
    ensure_available(bytes, offset, 4, "u32")?;
    Ok(u32::from_le_bytes(bytes[offset..offset + 4].try_into()?))
}
