use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::Path,
};

use anyhow::{Context, Result, bail};

mod event;
mod header;
mod relations;
mod writer;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/ftrace2parquet.rs"));
}

use event::parse_event;
use header::{HeaderParser, is_structured_header_line};
use relations::OutputTables;

const MAX_LINE_BYTES: usize = 1024 * 1024;

pub fn convert(input: &Path, output: &Path, clock_domain: &str) -> Result<()> {
    if clock_domain.is_empty() {
        bail!("clock domain must not be empty");
    }
    if output.exists() {
        bail!("output already exists: {}", output.display());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !fs::metadata(parent)
        .with_context(|| format!("failed to inspect output directory {}", parent.display()))?
        .is_dir()
    {
        bail!("output parent is not a directory: {}", parent.display());
    }
    let input_file =
        File::open(input).with_context(|| format!("failed to open input {}", input.display()))?;
    let temporary = tempfile::Builder::new()
        .prefix(".ftrace2parquet-")
        .tempdir_in(parent)
        .with_context(|| format!("failed to create temporary output in {}", parent.display()))?;
    let mut tables = OutputTables::new(temporary.path());
    if convert_reader(BufReader::new(input_file), clock_domain, &mut tables)? == 0 {
        bail!("text ftrace contains no supported event records");
    }
    tables.finish()?;
    fs::rename(temporary.path(), output)
        .with_context(|| format!("failed to publish output directory {}", output.display()))?;
    Ok(())
}

fn convert_reader(
    mut reader: impl BufRead,
    clock_domain: &str,
    tables: &mut OutputTables,
) -> Result<u64> {
    let mut bytes = Vec::new();
    let mut line_number = 0_u64;
    let mut source_sequence = 0_u64;
    let mut supported = 0_u64;
    let mut header_parser = Some(HeaderParser::default());
    let mut header = None;
    loop {
        let next_line = line_number
            .checked_add(1)
            .context("line number overflows")?;
        if read_bounded_line(&mut reader, &mut bytes, next_line)? == 0 {
            break;
        }
        line_number = next_line;
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        let line = std::str::from_utf8(&bytes)
            .with_context(|| format!("line {line_number} is not valid UTF-8"))?;
        let line = line.trim_start();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            if let Some(parser) = header_parser.as_mut() {
                parser
                    .consume(line, line_number)
                    .with_context(|| format!("invalid ftrace header at line {line_number}"))?;
            } else if is_structured_header_line(line) {
                bail!("ftrace header appears after events at line {line_number}");
            }
            continue;
        }
        if header.is_none() {
            let parsed = header_parser
                .take()
                .expect("header parser exists before first event")
                .finish()
                .with_context(|| format!("invalid ftrace header before line {line_number}"))?;
            header = Some(parsed);
        }
        let parsed_header = header.as_ref().expect("header parsed before event");
        if let Some(event) = parse_event(
            line,
            clock_domain,
            parsed_header.cpu_count,
            parsed_header.has_tgid_column,
        )
        .with_context(|| format!("invalid ftrace event at line {line_number}"))?
        {
            tables.push(source_sequence, event)?;
            supported = supported
                .checked_add(1)
                .context("supported event count overflows")?;
        }
        source_sequence = source_sequence
            .checked_add(1)
            .context("source event sequence overflows")?;
    }
    let header = match header {
        Some(header) => header,
        None => header_parser
            .take()
            .expect("header parser exists when no event was read")
            .finish()?,
    };
    if source_sequence != header.entries_in_buffer {
        bail!(
            "ftrace header declares {} buffered events, but text contains {source_sequence}",
            header.entries_in_buffer
        );
    }
    tables.set_header(header);
    Ok(supported)
}

fn read_bounded_line(
    reader: &mut impl BufRead,
    output: &mut Vec<u8>,
    line_number: u64,
) -> Result<usize> {
    output.clear();
    loop {
        let (take, has_newline) = {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return Ok(output.len());
            }
            let newline = available.iter().position(|b| *b == b'\n');
            let take = newline.map_or(available.len(), |p| p + 1);
            if output.len().saturating_add(take) > MAX_LINE_BYTES {
                bail!("line {line_number} exceeds the {MAX_LINE_BYTES}-byte limit");
            }
            output.extend_from_slice(&available[..take]);
            (take, newline.is_some())
        };
        reader.consume(take);
        if has_newline {
            return Ok(output.len());
        }
    }
}
