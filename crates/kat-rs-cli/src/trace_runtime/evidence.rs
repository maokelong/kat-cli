use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use anyhow::{Context, Result};
use serde_json::Value;

pub fn write_stdout(out: &mut dyn Write, evidence: &Value) -> Result<()> {
    serde_json::to_writer_pretty(&mut *out, evidence).context("failed to write evidence JSON")?;
    writeln!(out).context("failed to write evidence newline")?;
    Ok(())
}

pub fn append_jsonl(run_dir: impl AsRef<Path>, evidence: &Value) -> Result<()> {
    let run_dir = run_dir.as_ref();
    fs::create_dir_all(run_dir)
        .with_context(|| format!("failed to create run dir {}", run_dir.display()))?;

    let path = run_dir.join("evidence.jsonl");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open {}", path.display()))?;

    serde_json::to_writer(&mut file, evidence)
        .with_context(|| format!("failed to append {}", path.display()))?;
    writeln!(file).with_context(|| format!("failed to terminate {}", path.display()))?;
    Ok(())
}
