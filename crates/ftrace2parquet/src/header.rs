use anyhow::{Context, Result, bail};

use crate::event::{parse_u32, parse_u64};

#[derive(Clone, Debug)]
pub(crate) struct FtraceHeader {
    pub(crate) tracer: String,
    pub(crate) entries_in_buffer: u64,
    pub(crate) entries_written: u64,
    pub(crate) cpu_count: u32,
    pub(crate) has_tgid_column: bool,
}

#[derive(Default)]
pub(crate) struct HeaderParser {
    tracer: Option<String>,
    entries: Option<(u64, u64, u32)>,
    legends: u8,
    has_tgid_column: Option<bool>,
}

impl HeaderParser {
    pub(crate) fn consume(&mut self, line: &str, line_number: u64) -> Result<()> {
        let content = line
            .trim_start()
            .strip_prefix('#')
            .context("ftrace header line must start with '#'")?
            .trim();
        if let Some(value) = content.strip_prefix("tracer:") {
            if self.tracer.is_some() {
                bail!("duplicate tracer header at line {line_number}");
            }
            if self.entries.is_some() || self.legends != 0 || self.has_tgid_column.is_some() {
                bail!("tracer header is out of order at line {line_number}");
            }
            let value = value.trim();
            if value.is_empty() {
                bail!("empty tracer name at line {line_number}");
            }
            self.tracer = Some(value.to_owned());
            return Ok(());
        }
        if let Some(value) = content.strip_prefix("entries-in-buffer/entries-written:") {
            if self.tracer.is_none() {
                bail!("buffer header precedes tracer at line {line_number}");
            }
            if self.entries.is_some() {
                bail!("duplicate buffer header at line {line_number}");
            }
            if self.legends != 0 || self.has_tgid_column.is_some() {
                bail!("buffer header is out of order at line {line_number}");
            }
            let (counts, cpu_count) = value
                .split_once("#P:")
                .with_context(|| format!("invalid buffer header at line {line_number}"))?;
            let (buffered, written) = counts
                .trim()
                .split_once('/')
                .with_context(|| format!("invalid buffer counts at line {line_number}"))?;
            let buffered = parse_u64(buffered.trim(), "entries-in-buffer")
                .with_context(|| format!("invalid buffer header at line {line_number}"))?;
            let written = parse_u64(written.trim(), "entries-written")
                .with_context(|| format!("invalid buffer header at line {line_number}"))?;
            let cpu_count = parse_u32(cpu_count.trim(), "CPU count")
                .with_context(|| format!("invalid buffer header at line {line_number}"))?;
            if cpu_count == 0 {
                bail!("CPU count must be greater than zero at line {line_number}");
            }
            if buffered > written {
                bail!(
                    "entries-in-buffer {buffered} exceeds entries-written {written} at line {line_number}"
                );
            }
            self.entries = Some((buffered, written, cpu_count));
            return Ok(());
        }
        let legend = if content.contains("=> irqs-off") {
            Some(0)
        } else if content.contains("=> need-resched") {
            Some(1)
        } else if content.contains("=> hardirq/softirq") {
            Some(2)
        } else if content.contains("=> preempt-depth") {
            Some(3)
        } else if content.contains("=> migrate-disable") {
            Some(4)
        } else if content.contains("delay") && content.contains('/') {
            Some(5)
        } else {
            None
        };
        if let Some(legend) = legend {
            if self.entries.is_none() {
                bail!("flag legend precedes buffer header at line {line_number}");
            }
            let expected = match self.legends {
                0..=3 => self.legends,
                4 => {
                    if legend == 5 {
                        5
                    } else {
                        4
                    }
                }
                5 => 5,
                _ => bail!("duplicate flag legend at line {line_number}"),
            };
            if legend != expected {
                bail!("flag legend is out of order at line {line_number}");
            }
            self.legends = legend + 1;
            return Ok(());
        }
        if content.contains("TASK-PID")
            || content.contains("CPU#")
            || content.contains("TIMESTAMP")
            || content.contains("FUNCTION")
        {
            if self.has_tgid_column.is_some() {
                bail!("duplicate column header at line {line_number}");
            }
            if self.legends != 6 {
                bail!("column header precedes complete flag legend at line {line_number}");
            }
            let task = content
                .find("TASK-PID")
                .with_context(|| format!("column header lacks TASK-PID at line {line_number}"))?;
            let cpu = content
                .find("CPU#")
                .with_context(|| format!("column header lacks CPU# at line {line_number}"))?;
            let timestamp = content
                .find("TIMESTAMP")
                .with_context(|| format!("column header lacks TIMESTAMP at line {line_number}"))?;
            let function = content
                .find("FUNCTION")
                .with_context(|| format!("column header lacks FUNCTION at line {line_number}"))?;
            if !(task < cpu && cpu < timestamp && timestamp < function) {
                bail!("column header fields are out of order at line {line_number}");
            }
            let has_tgid = content.find("TGID");
            if has_tgid.is_some_and(|tgid| !(task < tgid && tgid < cpu)) {
                bail!("TGID column is out of order at line {line_number}");
            }
            self.has_tgid_column = Some(has_tgid.is_some());
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<FtraceHeader> {
        let mut missing = Vec::new();
        if self.tracer.is_none() {
            missing.push("tracer");
        }
        if self.entries.is_none() {
            missing.push("buffer counts and CPU count");
        }
        if self.legends != 6 {
            missing.push("complete context flag legend");
        }
        if self.has_tgid_column.is_none() {
            missing.push("event column header");
        }
        if !missing.is_empty() {
            bail!("invalid ftrace header: missing {}", missing.join(", "));
        }
        let (entries_in_buffer, entries_written, cpu_count) =
            self.entries.expect("header entries checked");
        Ok(FtraceHeader {
            tracer: self.tracer.expect("header tracer checked"),
            entries_in_buffer,
            entries_written,
            cpu_count,
            has_tgid_column: self.has_tgid_column.expect("header columns checked"),
        })
    }
}

pub(crate) fn is_structured_header_line(line: &str) -> bool {
    let content = line.trim_start_matches('#').trim();
    content.starts_with("tracer:")
        || content.starts_with("entries-in-buffer/entries-written:")
        || content.contains("TASK-PID")
        || content.contains("=> irqs-off")
}
