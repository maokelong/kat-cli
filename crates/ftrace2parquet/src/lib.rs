use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use arrow_schema::{DataType, Field, FieldRef, Schema};
use parquet::arrow::ArrowWriter;
use serde::{Deserialize, Serialize};
use serde_arrow::{
    ArrayBuilder,
    schema::{SchemaLike, TracingOptions},
};

const MAX_LINE_BYTES: usize = 1024 * 1024;
const BATCH_ROWS: usize = 8_192;
const TICKS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Clone, Debug)]
struct FtraceHeader {
    tracer: String,
    entries_in_buffer: u64,
    entries_written: u64,
    cpu_count: u32,
    has_tgid_column: bool,
}

#[derive(Default)]
struct HeaderParser {
    tracer: Option<String>,
    entries: Option<(u64, u64, u32)>,
    legends: u8,
    has_tgid_column: Option<bool>,
}

impl HeaderParser {
    fn consume(&mut self, line: &str, line_number: u64) -> Result<()> {
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

    fn finish(self) -> Result<FtraceHeader> {
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

mod generated {
    include!(concat!(env!("OUT_DIR"), "/ftrace2parquet.rs"));
}

use generated::{
    SchedSwitch, SchedWakeup, SchedWakeupNew, TextFtraceEvent, TracingMarkWrite,
    text_ftrace_event::Payload,
};

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

fn is_structured_header_line(line: &str) -> bool {
    let content = line.trim_start_matches('#').trim();
    content.starts_with("tracer:")
        || content.starts_with("entries-in-buffer/entries-written:")
        || content.contains("TASK-PID")
        || content.contains("=> irqs-off")
}

fn parse_event(
    line: &str,
    clock_domain: &str,
    header_cpu_count: u32,
    has_tgid_column: bool,
) -> Result<Option<TextFtraceEvent>> {
    let first_separator = line.find(": ").context("missing event separator")?;
    let cpu_start = line[..first_separator]
        .rfind(" [")
        .context("missing CPU field")?
        + 1;
    let cpu_end = line[cpu_start..].find("] ").context("invalid CPU field")? + cpu_start;
    let cpu = parse_u32(&line[cpu_start + 1..cpu_end], "CPU")?;
    if cpu >= header_cpu_count {
        bail!("CPU {cpu} is outside header CPU count {header_cpu_count}");
    }
    let emitter = line[..cpu_start].trim_end();
    let (flags_and_clock, event_and_payload) = line[cpu_end + 2..]
        .split_once(": ")
        .context("missing event name")?;
    let (context_flags, clock) = flags_and_clock
        .rsplit_once(char::is_whitespace)
        .context("missing context flags or clock value")?;
    if context_flags.is_empty() {
        bail!("context flags must not be empty");
    }
    let (event_name, payload_text) = event_and_payload
        .split_once(": ")
        .context("missing event payload")?;
    if event_name.is_empty() {
        bail!("event name must not be empty");
    }
    let (name, tid, tgid) = parse_emitter(emitter, has_tgid_column)?;
    let clock_value = parse_clock_value(clock)?;
    let payload = match event_name {
        "sched_switch" => Payload::SchedSwitch(parse_sched_switch(payload_text)?),
        "sched_wakeup" => Payload::SchedWakeup(parse_sched_wakeup(payload_text)?),
        "sched_wakeup_new" => Payload::SchedWakeupNew(parse_sched_wakeup_new(payload_text)?),
        "tracing_mark_write" => Payload::TracingMarkWrite(TracingMarkWrite {
            content: payload_text.to_owned(),
        }),
        _ => return Ok(None),
    };
    Ok(Some(TextFtraceEvent {
        clock_domain: clock_domain.to_owned(),
        clock_value,
        cpu,
        emitter_thread_name: name.to_owned(),
        emitter_thread_id: tid,
        emitter_process_id: tgid,
        context_flags: context_flags.to_owned(),
        payload: Some(payload),
    }))
}

fn parse_sched_switch(payload: &str) -> Result<SchedSwitch> {
    let (previous_thread_name, rest) = take_between(payload, "prev_comm=", " prev_pid=")?;
    let (previous_thread_id, rest) = take_between(rest, "", " prev_prio=")?;
    let (previous_priority, rest) = take_between(rest, "", " prev_state=")?;
    let (previous_state, rest) = take_between(rest, "", " ==> next_comm=")?;
    let (next_thread_name, rest) = take_between(rest, "", " next_pid=")?;
    let (next_thread_id, next_priority) = take_between(rest, "", " next_prio=")?;
    Ok(SchedSwitch {
        previous_thread_name: previous_thread_name.to_owned(),
        previous_thread_id: parse_i32(previous_thread_id, "prev_pid")?,
        previous_priority: parse_i32(previous_priority, "prev_prio")?,
        previous_state: previous_state.to_owned(),
        next_thread_name: next_thread_name.to_owned(),
        next_thread_id: parse_i32(next_thread_id, "next_pid")?,
        next_priority: parse_i32(next_priority, "next_prio")?,
    })
}

fn parse_sched_wakeup(payload: &str) -> Result<SchedWakeup> {
    let (thread_name, thread_id, priority, target_cpu) = parse_wakeup_fields(payload)?;
    Ok(SchedWakeup {
        thread_name,
        thread_id,
        priority,
        target_cpu,
    })
}

fn parse_sched_wakeup_new(payload: &str) -> Result<SchedWakeupNew> {
    let (thread_name, thread_id, priority, target_cpu) = parse_wakeup_fields(payload)?;
    Ok(SchedWakeupNew {
        thread_name,
        thread_id,
        priority,
        target_cpu,
    })
}

fn parse_wakeup_fields(payload: &str) -> Result<(String, i32, i32, u32)> {
    let (thread_name, rest) = take_between(payload, "comm=", " pid=")?;
    let (thread_id, rest) = take_between(rest, "", " prio=")?;
    let (priority, target_cpu) = take_between(rest, "", " target_cpu=")?;
    Ok((
        thread_name.to_owned(),
        parse_i32(thread_id, "pid")?,
        parse_i32(priority, "prio")?,
        parse_u32(target_cpu, "target_cpu")?,
    ))
}

fn take_between<'a>(value: &'a str, prefix: &str, separator: &str) -> Result<(&'a str, &'a str)> {
    value
        .strip_prefix(prefix)
        .context("missing payload field")?
        .split_once(separator)
        .context("missing payload separator")
}

fn parse_emitter(value: &str, has_tgid_column: bool) -> Result<(&str, i32, Option<i32>)> {
    let (thread, process) = if has_tgid_column {
        let close = value
            .strip_suffix(')')
            .context("missing TGID closing delimiter")?;
        let (thread, raw_process) = close.rsplit_once(" (").context("missing TGID field")?;
        let process = if raw_process.trim() == "-------" {
            None
        } else {
            Some(parse_i32(raw_process.trim(), "TGID")?)
        };
        (thread, process)
    } else {
        (value, None)
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
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        bail!("invalid fractional clock value");
    }
    if fraction.len() > 9 && fraction.as_bytes()[9..].iter().any(|d| *d != b'0') {
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
        .and_then(|v| v.checked_add(nanos))
        .context("clock value overflows UInt64")
}

fn parse_i32(value: &str, label: &str) -> Result<i32> {
    value.parse().with_context(|| format!("invalid {label}"))
}
fn parse_u32(value: &str, label: &str) -> Result<u32> {
    value.parse().with_context(|| format!("invalid {label}"))
}
fn parse_u64(value: &str, label: &str) -> Result<u64> {
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

#[derive(Serialize, Deserialize)]
struct OccurrenceRow {
    _kat_row_id: u64,
    source_event_sequence: u64,
}
#[derive(Serialize, Deserialize)]
struct RootRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    clock_domain: String,
    clock_value: u64,
    cpu: u32,
    emitter_thread_name: String,
    emitter_thread_id: i32,
    emitter_process_id: Option<i32>,
    context_flags: String,
}
#[derive(Serialize, Deserialize)]
struct SchedSwitchRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    previous_thread_name: String,
    previous_thread_id: i32,
    previous_priority: i32,
    previous_state: String,
    next_thread_name: String,
    next_thread_id: i32,
    next_priority: i32,
}
#[derive(Serialize, Deserialize)]
struct WakeupRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    thread_name: String,
    thread_id: i32,
    priority: i32,
    target_cpu: u32,
}
#[derive(Serialize, Deserialize)]
struct TracingMarkWriteRow {
    _kat_row_id: u64,
    _kat_parent_row_id: u64,
    content: String,
}
#[derive(Serialize, Deserialize)]
struct HeaderRow {
    tracer: String,
    entries_in_buffer: u64,
    entries_written: u64,
    cpu_count: u32,
    has_tgid_column: bool,
}

struct TableWriter<T> {
    name: &'static str,
    builder: ArrayBuilder,
    writer: ArrowWriter<File>,
    buffered_rows: usize,
    _row: PhantomData<T>,
}
impl<T> TableWriter<T>
where
    for<'de> T: Deserialize<'de>,
    T: Serialize,
{
    fn new(directory: &Path, name: &'static str) -> Result<Self> {
        let fields = Vec::<FieldRef>::from_type::<T>(TracingOptions::default())?
            .into_iter()
            .map(|field| {
                if field.data_type() == &DataType::LargeUtf8 {
                    Arc::new(Field::new(
                        field.name(),
                        DataType::Utf8,
                        field.is_nullable(),
                    ))
                } else {
                    field
                }
            })
            .collect::<Vec<_>>();
        let schema = Arc::new(Schema::new(fields.clone()));
        let file = File::create(directory.join(format!("{name}.parquet")))?;
        Ok(Self {
            name,
            builder: ArrayBuilder::from_arrow(&fields)?,
            writer: ArrowWriter::try_new(file, schema, None)?,
            buffered_rows: 0,
            _row: PhantomData,
        })
    }
    fn push(&mut self, row: T) -> Result<()> {
        self.builder.push(row)?;
        self.buffered_rows += 1;
        if self.buffered_rows == BATCH_ROWS {
            self.flush()?;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<()> {
        if self.buffered_rows == 0 {
            return Ok(());
        }
        let batch = self
            .builder
            .to_record_batch()
            .with_context(|| format!("failed to build {:?} batch", self.name))?;
        self.writer
            .write(&batch)
            .with_context(|| format!("failed to write {:?} batch", self.name))?;
        self.buffered_rows = 0;
        Ok(())
    }
    fn finish(mut self) -> Result<()> {
        self.flush()?;
        self.writer
            .close()
            .with_context(|| format!("failed to close {:?} table", self.name))?;
        Ok(())
    }
}

struct OutputTables {
    directory: PathBuf,
    occurrence: Option<TableWriter<OccurrenceRow>>,
    root: Option<TableWriter<RootRow>>,
    sched_switch: Option<TableWriter<SchedSwitchRow>>,
    sched_wakeup: Option<TableWriter<WakeupRow>>,
    sched_wakeup_new: Option<TableWriter<WakeupRow>>,
    tracing_mark_write: Option<TableWriter<TracingMarkWriteRow>>,
    header: Option<FtraceHeader>,
    next_root: u64,
    next_switch: u64,
    next_wakeup: u64,
    next_wakeup_new: u64,
    next_marker: u64,
}
impl OutputTables {
    fn new(directory: &Path) -> Self {
        Self {
            directory: directory.to_owned(),
            occurrence: None,
            root: None,
            sched_switch: None,
            sched_wakeup: None,
            sched_wakeup_new: None,
            tracing_mark_write: None,
            header: None,
            next_root: 0,
            next_switch: 0,
            next_wakeup: 0,
            next_wakeup_new: 0,
            next_marker: 0,
        }
    }
    fn set_header(&mut self, header: FtraceHeader) {
        self.header = Some(header);
    }
    fn push(&mut self, source_event_sequence: u64, event: TextFtraceEvent) -> Result<()> {
        let root_id = take_next(&mut self.next_root)?;
        self.occurrence()?.push(OccurrenceRow {
            _kat_row_id: root_id,
            source_event_sequence,
        })?;
        let payload = event.payload.context("supported event has no payload")?;
        self.root()?.push(RootRow {
            _kat_row_id: root_id,
            _kat_parent_row_id: root_id,
            clock_domain: event.clock_domain,
            clock_value: event.clock_value,
            cpu: event.cpu,
            emitter_thread_name: event.emitter_thread_name,
            emitter_thread_id: event.emitter_thread_id,
            emitter_process_id: event.emitter_process_id,
            context_flags: event.context_flags,
        })?;
        match payload {
            Payload::SchedSwitch(v) => {
                let id = take_next(&mut self.next_switch)?;
                self.sched_switch()?.push(SchedSwitchRow {
                    _kat_row_id: id,
                    _kat_parent_row_id: root_id,
                    previous_thread_name: v.previous_thread_name,
                    previous_thread_id: v.previous_thread_id,
                    previous_priority: v.previous_priority,
                    previous_state: v.previous_state,
                    next_thread_name: v.next_thread_name,
                    next_thread_id: v.next_thread_id,
                    next_priority: v.next_priority,
                })?;
            }
            Payload::SchedWakeup(v) => {
                let id = take_next(&mut self.next_wakeup)?;
                self.sched_wakeup()?.push(WakeupRow {
                    _kat_row_id: id,
                    _kat_parent_row_id: root_id,
                    thread_name: v.thread_name,
                    thread_id: v.thread_id,
                    priority: v.priority,
                    target_cpu: v.target_cpu,
                })?;
            }
            Payload::SchedWakeupNew(v) => {
                let id = take_next(&mut self.next_wakeup_new)?;
                self.sched_wakeup_new()?.push(WakeupRow {
                    _kat_row_id: id,
                    _kat_parent_row_id: root_id,
                    thread_name: v.thread_name,
                    thread_id: v.thread_id,
                    priority: v.priority,
                    target_cpu: v.target_cpu,
                })?;
            }
            Payload::TracingMarkWrite(v) => {
                let id = take_next(&mut self.next_marker)?;
                self.tracing_mark_write()?.push(TracingMarkWriteRow {
                    _kat_row_id: id,
                    _kat_parent_row_id: root_id,
                    content: v.content,
                })?;
            }
        }
        Ok(())
    }
    fn occurrence(&mut self) -> Result<&mut TableWriter<OccurrenceRow>> {
        initialize(
            &self.directory,
            &mut self.occurrence,
            "text_ftrace_event_occurrence",
        )
    }
    fn root(&mut self) -> Result<&mut TableWriter<RootRow>> {
        initialize(&self.directory, &mut self.root, "text_ftrace_event")
    }
    fn sched_switch(&mut self) -> Result<&mut TableWriter<SchedSwitchRow>> {
        initialize(
            &self.directory,
            &mut self.sched_switch,
            "text_ftrace_event_sched_switch",
        )
    }
    fn sched_wakeup(&mut self) -> Result<&mut TableWriter<WakeupRow>> {
        initialize(
            &self.directory,
            &mut self.sched_wakeup,
            "text_ftrace_event_sched_wakeup",
        )
    }
    fn sched_wakeup_new(&mut self) -> Result<&mut TableWriter<WakeupRow>> {
        initialize(
            &self.directory,
            &mut self.sched_wakeup_new,
            "text_ftrace_event_sched_wakeup_new",
        )
    }
    fn tracing_mark_write(&mut self) -> Result<&mut TableWriter<TracingMarkWriteRow>> {
        initialize(
            &self.directory,
            &mut self.tracing_mark_write,
            "text_ftrace_event_tracing_mark_write",
        )
    }
    fn finish(self) -> Result<()> {
        let header = self.header.context("validated ftrace header is missing")?;
        let mut header_table =
            TableWriter::<HeaderRow>::new(&self.directory, "text_ftrace_header")?;
        header_table.push(HeaderRow {
            tracer: header.tracer,
            entries_in_buffer: header.entries_in_buffer,
            entries_written: header.entries_written,
            cpu_count: header.cpu_count,
            has_tgid_column: header.has_tgid_column,
        })?;
        header_table.finish()?;
        finish(self.occurrence)?;
        finish(self.root)?;
        finish(self.sched_switch)?;
        finish(self.sched_wakeup)?;
        finish(self.sched_wakeup_new)?;
        finish(self.tracing_mark_write)?;
        Ok(())
    }
}

fn initialize<'a, T>(
    directory: &Path,
    table: &'a mut Option<TableWriter<T>>,
    name: &'static str,
) -> Result<&'a mut TableWriter<T>>
where
    for<'de> T: Deserialize<'de>,
    T: Serialize,
{
    if table.is_none() {
        *table = Some(TableWriter::new(directory, name)?);
    }
    Ok(table.as_mut().expect("table initialized"))
}
fn finish<T>(table: Option<TableWriter<T>>) -> Result<()>
where
    for<'de> T: Deserialize<'de>,
    T: Serialize,
{
    if let Some(table) = table {
        table.finish()?;
    }
    Ok(())
}
fn take_next(value: &mut u64) -> Result<u64> {
    let current = *value;
    *value = value.checked_add(1).context("row id overflows")?;
    Ok(current)
}
