use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use arrow_array::{Int32Array, RecordBatch, StringArray, UInt32Array, UInt64Array};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use parquet::arrow::ArrowWriter;

const MAX_LINE_BYTES: usize = 1024 * 1024;
const BATCH_ROWS: usize = 8_192;
const TICKS_PER_SECOND: u64 = 1_000_000_000;

pub fn convert(input: &Path, output: &Path, clock_domain: &str) -> Result<()> {
    if clock_domain.is_empty() {
        bail!("clock domain must not be empty");
    }
    if output.exists() {
        bail!("output already exists: {}", output.display());
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let metadata = fs::metadata(parent)
        .with_context(|| format!("failed to inspect output directory {}", parent.display()))?;
    if !metadata.is_dir() {
        bail!("output parent is not a directory: {}", parent.display());
    }

    let input_file =
        File::open(input).with_context(|| format!("failed to open input {}", input.display()))?;
    let temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temporary output in {}", parent.display()))?;
    let schema = event_schema();
    let parquet_file = temporary
        .reopen()
        .context("failed to reopen temporary output")?;
    let mut writer = ArrowWriter::try_new(parquet_file, schema.clone(), None)
        .context("failed to create Parquet writer")?;
    let event_count = convert_reader(
        BufReader::new(input_file),
        clock_domain,
        schema,
        &mut writer,
    )?;
    if event_count == 0 {
        bail!("text ftrace contains no event records");
    }
    writer.close().context("failed to finish Parquet output")?;
    temporary.persist_noclobber(output).map_err(|error| {
        anyhow::anyhow!(
            "failed to publish output {}: {}",
            output.display(),
            error.error
        )
    })?;
    Ok(())
}

fn convert_reader(
    mut reader: impl BufRead,
    clock_domain: &str,
    schema: SchemaRef,
    writer: &mut ArrowWriter<File>,
) -> Result<u64> {
    let mut bytes = Vec::new();
    let mut line_number = 0_u64;
    let mut sequence = 0_u64;
    let mut batch = EventBatch::new();
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
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let event = parse_event(line)
            .with_context(|| format!("invalid ftrace event at line {line_number}"))?;
        batch.push(sequence, clock_domain, event);
        sequence = sequence
            .checked_add(1)
            .context("source event sequence overflows")?;
        if batch.len() == BATCH_ROWS {
            writer
                .write(&batch.finish(schema.clone())?)
                .context("failed to write Parquet batch")?;
        }
    }
    if !batch.is_empty() {
        writer
            .write(&batch.finish(schema)?)
            .context("failed to write final Parquet batch")?;
    }
    Ok(sequence)
}

struct ParsedEvent<'a> {
    clock_value: u64,
    cpu: u32,
    emitter_thread_name: &'a str,
    emitter_thread_id: i32,
    emitter_process_id: Option<i32>,
    context_flags: &'a str,
    event_name: &'a str,
    payload: &'a str,
}

fn parse_event(line: &str) -> Result<ParsedEvent<'_>> {
    let first_separator = line.find(": ").context("missing event separator")?;
    let cpu_start = line[..first_separator]
        .rfind(" [")
        .context("missing CPU field")?
        + 1;
    let cpu_end = line[cpu_start..].find("] ").context("invalid CPU field")? + cpu_start;
    let cpu = parse_u32(&line[cpu_start + 1..cpu_end], "CPU")?;
    let emitter = line[..cpu_start].trim_end();
    let suffix = &line[cpu_end + 2..];
    let (flags_and_clock, event_and_payload) =
        suffix.split_once(": ").context("missing event name")?;
    let (context_flags, clock) = flags_and_clock
        .rsplit_once(char::is_whitespace)
        .context("missing context flags or clock value")?;
    if context_flags.is_empty() {
        bail!("context flags must not be empty");
    }
    let (event_name, payload) = event_and_payload
        .split_once(": ")
        .context("missing event payload")?;
    if event_name.is_empty() {
        bail!("event name must not be empty");
    }
    let (emitter_thread_name, emitter_thread_id, emitter_process_id) = parse_emitter(emitter)?;
    Ok(ParsedEvent {
        clock_value: parse_clock_value(clock)?,
        cpu,
        emitter_thread_name,
        emitter_thread_id,
        emitter_process_id,
        context_flags,
        event_name,
        payload,
    })
}

fn parse_emitter(value: &str) -> Result<(&str, i32, Option<i32>)> {
    let close = value
        .strip_suffix(')')
        .context("missing TGID closing delimiter")?;
    let (thread, raw_process) = close.rsplit_once(" (").context("missing TGID field")?;
    let process = if raw_process.trim() == "-------" {
        None
    } else {
        Some(parse_i32(raw_process.trim(), "TGID")?)
    };
    let (name, raw_thread_id) = thread
        .rsplit_once('-')
        .context("missing emitter thread ID")?;
    let name = name.trim();
    if name.is_empty() {
        bail!("emitter thread name must not be empty");
    }
    Ok((
        name,
        parse_i32(raw_thread_id.trim(), "emitter thread ID")?,
        process,
    ))
}

fn parse_clock_value(value: &str) -> Result<u64> {
    let (seconds, fraction) = value.split_once('.').unwrap_or((value, ""));
    let seconds = seconds.parse::<u64>().context("invalid clock seconds")?;
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("invalid fractional clock value");
    }
    if fraction.len() > 9 && fraction.as_bytes()[9..].iter().any(|digit| *digit != b'0') {
        bail!("clock precision exceeds nanoseconds");
    }
    let significant = &fraction[..fraction.len().min(9)];
    let mut nanos = if significant.is_empty() {
        0
    } else {
        significant
            .parse::<u64>()
            .context("invalid fractional clock value")?
    };
    for _ in significant.len()..9 {
        nanos *= 10;
    }
    seconds
        .checked_mul(TICKS_PER_SECOND)
        .and_then(|value| value.checked_add(nanos))
        .context("clock value overflows UInt64")
}

fn parse_i32(value: &str, label: &str) -> Result<i32> {
    value.parse().with_context(|| format!("invalid {label}"))
}

fn parse_u32(value: &str, label: &str) -> Result<u32> {
    value.parse().with_context(|| format!("invalid {label}"))
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
            let newline = available.iter().position(|byte| *byte == b'\n');
            let take = newline.map_or(available.len(), |position| position + 1);
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

fn event_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("source_event_sequence", DataType::UInt64, false),
        Field::new("clock_domain", DataType::Utf8, false),
        Field::new("clock_value", DataType::UInt64, false),
        Field::new("cpu", DataType::UInt32, false),
        Field::new("emitter_thread_name", DataType::Utf8, false),
        Field::new("emitter_thread_id", DataType::Int32, false),
        Field::new("emitter_process_id", DataType::Int32, true),
        Field::new("context_flags", DataType::Utf8, false),
        Field::new("event_name", DataType::Utf8, false),
        Field::new("payload", DataType::Utf8, false),
    ]))
}

#[derive(Default)]
struct EventBatch {
    source_event_sequence: Vec<u64>,
    clock_domain: Vec<String>,
    clock_value: Vec<u64>,
    cpu: Vec<u32>,
    emitter_thread_name: Vec<String>,
    emitter_thread_id: Vec<i32>,
    emitter_process_id: Vec<Option<i32>>,
    context_flags: Vec<String>,
    event_name: Vec<String>,
    payload: Vec<String>,
}

impl EventBatch {
    fn new() -> Self {
        Self::default()
    }

    fn len(&self) -> usize {
        self.source_event_sequence.len()
    }

    fn is_empty(&self) -> bool {
        self.source_event_sequence.is_empty()
    }

    fn push(&mut self, sequence: u64, clock_domain: &str, event: ParsedEvent<'_>) {
        self.source_event_sequence.push(sequence);
        self.clock_domain.push(clock_domain.to_owned());
        self.clock_value.push(event.clock_value);
        self.cpu.push(event.cpu);
        self.emitter_thread_name
            .push(event.emitter_thread_name.to_owned());
        self.emitter_thread_id.push(event.emitter_thread_id);
        self.emitter_process_id.push(event.emitter_process_id);
        self.context_flags.push(event.context_flags.to_owned());
        self.event_name.push(event.event_name.to_owned());
        self.payload.push(event.payload.to_owned());
    }

    fn finish(&mut self, schema: SchemaRef) -> Result<RecordBatch> {
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(std::mem::take(
                    &mut self.source_event_sequence,
                ))),
                Arc::new(StringArray::from(std::mem::take(&mut self.clock_domain))),
                Arc::new(UInt64Array::from(std::mem::take(&mut self.clock_value))),
                Arc::new(UInt32Array::from(std::mem::take(&mut self.cpu))),
                Arc::new(StringArray::from(std::mem::take(
                    &mut self.emitter_thread_name,
                ))),
                Arc::new(Int32Array::from(std::mem::take(
                    &mut self.emitter_thread_id,
                ))),
                Arc::new(Int32Array::from(std::mem::take(
                    &mut self.emitter_process_id,
                ))),
                Arc::new(StringArray::from(std::mem::take(&mut self.context_flags))),
                Arc::new(StringArray::from(std::mem::take(&mut self.event_name))),
                Arc::new(StringArray::from(std::mem::take(&mut self.payload))),
            ],
        )
        .context("failed to build Arrow batch")
    }
}
