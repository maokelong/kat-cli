//! Provides scoped file mapping so build-time parsing releases the trace file.

use std::{fs::File, path::Path};

use anyhow::{Context, Result};
use memmap2::MmapOptions;

pub(crate) fn with_mapped_file<T>(path: &Path, read: impl FnOnce(&[u8]) -> Result<T>) -> Result<T> {
    let file = File::open(path)
        .with_context(|| format!("failed to open trace file: {}", path.display()))?;
    let mmap = unsafe { MmapOptions::new().map(&file) }
        .with_context(|| format!("failed to memory map trace file: {}", path.display()))?;

    read(mmap.as_ref())
}
