// 文件映射封装成闭包作用域，确保构建 datasource 后能及时释放 trace 文件句柄。

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
