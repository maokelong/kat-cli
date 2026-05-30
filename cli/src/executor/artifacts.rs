use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

pub fn ensure_run_dir(path: &Path) -> Result<PathBuf> {
    fs::create_dir_all(path)?;
    Ok(path.to_path_buf())
}
