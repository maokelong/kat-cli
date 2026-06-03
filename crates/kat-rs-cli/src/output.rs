use anyhow::Result;
use serde::Serialize;
use std::io::{self, Write};

pub fn write_json_pretty(value: &impl Serialize) -> Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, value)?;
    writeln!(handle)?;
    Ok(())
}
