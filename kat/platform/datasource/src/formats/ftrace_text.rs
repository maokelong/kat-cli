use std::{collections::BTreeMap, io::BufRead};

use crate::proto::kat::hitrace::{
    self as p, FtraceEvent, PrintFormat, SchedSwitchFormat, SchedWakeupFormat,
    SchedWakeupNewFormat, ftrace_event,
};
use anyhow::{Context, Result, bail};

const MAX_LINE_BYTES: usize = 1024 * 1024;
const TICKS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Debug, thiserror::Error)]
#[error("incompatible {event}.{field} at line {line}: {reason}")]
pub struct TextFtraceCompatibilityError {
    event: String,
    field: String,
    line: u64,
    reason: String,
}

impl TextFtraceCompatibilityError {
    pub fn event(&self) -> &str {
        &self.event
    }

    pub fn field(&self) -> &str {
        &self.field
    }

    pub fn line(&self) -> u64 {
        self.line
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{reason}")]
struct FieldParseError {
    field: &'static str,
    reason: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub enum TextFtraceClock {
    Boottime,
    Monotonic,
    FtraceGlobal,
}

fn parse_additional_event(name: &str, payload: &str) -> Option<Result<ftrace_event::Event>> {
    match name {
        "binder_transaction" => Some(parse_binder_transaction(payload)),
        "binder_transaction_received" => Some(parse_binder_received(payload)),
        "block_bio_remap" => Some(parse_block_bio_remap(payload)),
        "block_rq_complete" => Some(parse_block_complete(payload)),
        "block_rq_insert" => Some(parse_block_request(payload, false)),
        "block_rq_issue" => Some(parse_block_request(payload, true)),
        "cpu_idle" => Some(parse_cpu_idle(payload)),
        "ipi_entry" => Some(parse_ipi_reason(payload, false)),
        "ipi_exit" => Some(parse_ipi_reason(payload, true)),
        "ipi_raise" => Some(parse_ipi_raise(payload)),
        "ipi_send_cpu" => Some(parse_ipi_send_cpu(payload)),
        "irq_handler_entry" => Some(parse_irq_entry(payload)),
        "irq_handler_exit" => Some(parse_irq_exit(payload)),
        "mm_vmscan_kswapd_sleep" => Some(parse_kswapd_sleep(payload)),
        "mm_vmscan_kswapd_wake" => Some(parse_kswapd_wake(payload)),
        "rss_stat" => Some(parse_rss_stat(payload)),
        "softirq_entry" => Some(parse_softirq(payload, SoftirqKind::Entry)),
        "softirq_exit" => Some(parse_softirq(payload, SoftirqKind::Exit)),
        "softirq_raise" => Some(parse_softirq(payload, SoftirqKind::Raise)),
        "workqueue_execute_end" => Some(parse_workqueue(payload, false)),
        "workqueue_execute_start" => Some(parse_workqueue(payload, true)),
        _ => None,
    }
}

fn encode_device(major: u32, minor: u32) -> u64 {
    (u64::from(major) << 20) | u64::from(minor)
}

fn parse_sched_state(value: &str) -> Result<u64> {
    let state = value.trim_end_matches('+');
    match state {
        "R" => Ok(0),
        "S" => Ok(1),
        "D" => Ok(2),
        "T" => Ok(4),
        "t" => Ok(8),
        "X" => Ok(16),
        "Z" => Ok(32),
        "P" => Ok(64),
        "I" => Ok(128),
        _ => bail!("unsupported sched_switch prev_state {value:?}"),
    }
}

impl TextFtraceClock {
    pub(crate) fn domain(self) -> &'static str {
        match self {
            Self::Boottime => "boottime",
            Self::Monotonic => "monotonic",
            Self::FtraceGlobal => "ftrace_global",
        }
    }

    pub(crate) fn proto_clock(self) -> &'static str {
        match self {
            Self::Boottime => "boot",
            Self::Monotonic => "mono",
            Self::FtraceGlobal => "global",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct UnsupportedFtraceEvent {
    name: String,
    count: u64,
    first_line: u64,
}

impl UnsupportedFtraceEvent {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn first_line(&self) -> u64 {
        self.first_line
    }
}

#[derive(Clone, Debug)]
struct CommonEventFields {
    clock_value: u64,
    cpu: u32,
    emitter_thread_name: String,
    emitter_thread_id: i32,
    emitter_process_id: Option<i32>,
}

pub(crate) struct TextFtraceSummary {
    unsupported: BTreeMap<String, (u64, u64)>,
    seen_events: u64,
}

impl TextFtraceSummary {
    pub(crate) fn unsupported_events(&self) -> Vec<UnsupportedFtraceEvent> {
        self.unsupported
            .iter()
            .map(|(name, (count, first_line))| UnsupportedFtraceEvent {
                name: name.clone(),
                count: *count,
                first_line: *first_line,
            })
            .collect()
    }

    fn finish(self) -> Result<Self> {
        if self.seen_events == 0 {
            bail!("text ftrace contains no event records");
        }
        Ok(self)
    }
}

pub(crate) fn decode_reader(
    mut reader: impl BufRead,
    mut emit: impl FnMut(u32, FtraceEvent) -> Result<()>,
) -> Result<TextFtraceSummary> {
    let mut bytes = Vec::new();
    let mut line_number = 0_u64;
    let mut summary = TextFtraceSummary {
        unsupported: BTreeMap::new(),
        seen_events: 0,
    };
    loop {
        let next_line = line_number
            .checked_add(1)
            .context("source line number overflows")?;
        let read = read_bounded_line(&mut reader, &mut bytes, next_line)?;
        if read == 0 {
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
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        parse_line(line.trim_start(), line_number, &mut summary, &mut emit)
            .with_context(|| format!("invalid ftrace event at line {line_number}"))?;
    }
    summary.finish()
}

fn read_bounded_line(
    reader: &mut impl BufRead,
    output: &mut Vec<u8>,
    line_number: u64,
) -> Result<usize> {
    output.clear();
    loop {
        let (take, newline) = {
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
        if newline {
            return Ok(output.len());
        }
    }
}

fn parse_line(
    line: &str,
    line_number: u64,
    summary: &mut TextFtraceSummary,
    emit: &mut impl FnMut(u32, FtraceEvent) -> Result<()>,
) -> Result<()> {
    let header_end = line.find(": ").context("missing event name")?;
    let cpu_start = line[..header_end]
        .rfind(" [")
        .context("missing CPU field")?
        + 1;
    let cpu_end = line[cpu_start..].find("] ").context("invalid CPU field")? + cpu_start;
    let emitter = line[..cpu_start].trim_end();
    let cpu = parse_u32(&line[cpu_start + 1..cpu_end], "CPU")?;
    let suffix = &line[cpu_end + 2..];
    let (flags_and_time, event_and_payload) =
        suffix.split_once(": ").context("missing event name")?;
    let (context_flags, seconds) = flags_and_time
        .rsplit_once(char::is_whitespace)
        .context("missing context flags or clock value")?;
    if context_flags.is_empty() {
        bail!("context flags must be nonempty");
    }
    let (event_name, payload) = event_and_payload
        .split_once(": ")
        .context("missing event payload")?;
    if event_name.is_empty() || payload.is_empty() {
        bail!("event name and payload must be nonempty");
    }
    let (emitter_thread_name, emitter_thread_id, emitter_process_id) = parse_emitter(emitter)?;
    let common = CommonEventFields {
        clock_value: parse_clock_value(seconds)?,
        cpu,
        emitter_thread_name,
        emitter_thread_id,
        emitter_process_id,
    };
    let event = if let Some(event) = parse_known_event(event_name, payload) {
        Some(event.map_err(|source| compatibility_error(event_name, line_number, source))?)
    } else {
        let entry = summary
            .unsupported
            .entry(event_name.to_owned())
            .or_insert((0, line_number));
        entry.0 = entry
            .0
            .checked_add(1)
            .context("unsupported event count overflows")?;
        None
    };
    if let Some(event) = event {
        emit(common.cpu, build_event(common, event)?)?;
    }
    summary.seen_events = summary
        .seen_events
        .checked_add(1)
        .context("event count overflows")?;
    Ok(())
}

fn parse_known_event(name: &str, payload: &str) -> Option<Result<ftrace_event::Event>> {
    match name {
        "sched_switch" => Some(parse_switch(payload)),
        "sched_wakeup" => Some(parse_wakeup(payload, false)),
        "sched_wakeup_new" => Some(parse_wakeup(payload, true)),
        "tracing_mark_write" => Some(Ok(ftrace_event::Event::PrintFormat(PrintFormat {
            ip: None,
            buf: payload.to_owned(),
        }))),
        _ => parse_additional_event(name, payload),
    }
}

fn compatibility_error(
    event: &str,
    line: u64,
    source: anyhow::Error,
) -> TextFtraceCompatibilityError {
    let field_error = source
        .chain()
        .find_map(|error| error.downcast_ref::<FieldParseError>());
    TextFtraceCompatibilityError {
        event: event.to_owned(),
        field: field_error
            .map_or("payload", |error| error.field)
            .to_owned(),
        line,
        reason: field_error.map_or_else(
            || source.root_cause().to_string(),
            |error| error.reason.to_owned(),
        ),
    }
}

fn parse_emitter(value: &str) -> Result<(String, i32, Option<i32>)> {
    let close = value.ends_with(')').then_some(value.len() - 1);
    let open = close.and_then(|_| value.rfind(" ("));
    let (thread, process) = match (open, close) {
        (Some(open), Some(close)) => {
            let raw = value[open + 2..close].trim();
            let process = if raw == "-------" {
                None
            } else {
                Some(parse_i32(raw, "TGID")?)
            };
            (value[..open].trim_end(), process)
        }
        _ => bail!("missing TGID field"),
    };
    let split = thread.rfind('-').context("missing emitter thread ID")?;
    let name = thread[..split].trim();
    if name.is_empty() {
        bail!("emitter thread name is empty");
    }
    Ok((
        name.to_owned(),
        parse_i32(&thread[split + 1..], "emitter thread ID")?,
        process,
    ))
}

fn parse_clock_value(value: &str) -> Result<u64> {
    let (seconds, fraction) = value.split_once('.').unwrap_or((value, ""));
    let seconds = seconds
        .parse::<u64>()
        .context("invalid integral clock value")?;
    if fraction.len() > 9 && fraction.as_bytes()[9..].iter().any(|digit| *digit != b'0') {
        bail!("clock value has nonzero precision beyond nanoseconds");
    }
    let significant = &fraction[..fraction.len().min(9)];
    if !significant.bytes().all(|digit| digit.is_ascii_digit()) {
        bail!("invalid fractional clock value");
    }
    let mut nanos = if significant.is_empty() {
        0
    } else {
        significant.parse::<u64>()?
    };
    for _ in significant.len()..9 {
        nanos = nanos
            .checked_mul(10)
            .context("fractional clock value overflows")?;
    }
    seconds
        .checked_mul(TICKS_PER_SECOND)
        .and_then(|value| value.checked_add(nanos))
        .context("clock value overflows UInt64")
}

fn parse_switch(payload: &str) -> Result<ftrace_event::Event> {
    let (previous_thread_name, rest) = take_between(payload, "prev_comm=", " prev_pid=")?;
    let (previous_thread_id, rest) = take_between(rest, "", " prev_prio=")?;
    let (previous_priority, rest) = take_between(rest, "", " prev_state=")?;
    let (previous_state, rest) = take_between(rest, "", " ==> next_comm=")?;
    let (next_thread_name, rest) = take_between(rest, "", " next_pid=")?;
    let (next_thread_id, next_priority) = take_between(rest, "", " next_prio=")?;
    Ok(ftrace_event::Event::SchedSwitchFormat(SchedSwitchFormat {
        prev_comm: previous_thread_name.to_owned(),
        prev_pid: parse_i32(previous_thread_id, "prev_pid")?,
        prev_prio: parse_i32(previous_priority, "prev_prio")?,
        prev_state: parse_sched_state(previous_state)?,
        next_comm: next_thread_name.to_owned(),
        next_pid: parse_i32(next_thread_id, "next_pid")?,
        next_prio: parse_i32(next_priority, "next_prio")?,
    }))
}

fn parse_wakeup(payload: &str, new: bool) -> Result<ftrace_event::Event> {
    let (thread_name, rest) = take_between(payload, "comm=", " pid=")?;
    let (thread_id, rest) = take_between(rest, "", " prio=")?;
    let (priority, target_cpu) = take_between(rest, "", " target_cpu=")?;
    let comm = thread_name.to_owned();
    let pid = parse_i32(thread_id, "pid")?;
    let prio = parse_i32(priority, "prio")?;
    let target_cpu = i32::try_from(parse_u32(target_cpu, "target_cpu")?)?;
    Ok(if new {
        ftrace_event::Event::SchedWakeupNewFormat(SchedWakeupNewFormat {
            comm,
            pid,
            prio,
            success: None,
            target_cpu,
        })
    } else {
        ftrace_event::Event::SchedWakeupFormat(SchedWakeupFormat {
            comm,
            pid,
            prio,
            success: None,
            target_cpu,
        })
    })
}

fn build_event(common: CommonEventFields, event: ftrace_event::Event) -> Result<FtraceEvent> {
    Ok(FtraceEvent {
        timestamp: common.clock_value,
        tgid: common.emitter_process_id,
        comm: common.emitter_thread_name,
        common_fields: Some(ftrace_event::CommonFileds {
            r#type: None,
            flags: None,
            preempt_count: None,
            pid: common.emitter_thread_id,
        }),
        event: Some(event),
    })
}

fn take_between<'a>(value: &'a str, prefix: &str, separator: &str) -> Result<(&'a str, &'a str)> {
    let value = value
        .strip_prefix(prefix)
        .context("missing payload field")?;
    value
        .split_once(separator)
        .context("missing payload separator")
}

fn parse_i32(value: &str, label: &'static str) -> Result<i32> {
    value.parse().map_err(|_| {
        FieldParseError {
            field: label,
            reason: "invalid signed 32-bit integer",
        }
        .into()
    })
}

fn parse_u32(value: &str, label: &'static str) -> Result<u32> {
    value.parse().map_err(|_| {
        FieldParseError {
            field: label,
            reason: "invalid unsigned 32-bit integer",
        }
        .into()
    })
}

fn parse_u64(value: &str, label: &'static str) -> Result<u64> {
    value.parse().map_err(|_| {
        FieldParseError {
            field: label,
            reason: "invalid unsigned 64-bit integer",
        }
        .into()
    })
}

fn parse_i64(value: &str, label: &'static str) -> Result<i64> {
    value.parse().map_err(|_| {
        FieldParseError {
            field: label,
            reason: "invalid signed 64-bit integer",
        }
        .into()
    })
}

fn parse_hex_u32(value: &str, label: &'static str) -> Result<u32> {
    let Some(digits) = value.strip_prefix("0x") else {
        return Err(FieldParseError {
            field: label,
            reason: "missing hexadecimal prefix",
        }
        .into());
    };
    u32::from_str_radix(digits, 16).map_err(|_| {
        FieldParseError {
            field: label,
            reason: "invalid unsigned 32-bit hexadecimal integer",
        }
        .into()
    })
}

fn parse_hex_u64(value: &str, label: &'static str) -> Result<u64> {
    let digits = value.strip_prefix("0x").unwrap_or(value);
    u64::from_str_radix(digits, 16).map_err(|_| {
        FieldParseError {
            field: label,
            reason: "invalid unsigned 64-bit hexadecimal integer",
        }
        .into()
    })
}

fn parse_device(value: &str) -> Result<(u32, u32)> {
    let (major, minor) = value.split_once(',').context("missing device separator")?;
    Ok((
        parse_u32(major, "device major")?,
        parse_u32(minor, "device minor")?,
    ))
}

fn exactly<'a>(tokens: &'a [&'a str], count: usize) -> Result<()> {
    if tokens.len() != count {
        bail!("expected {count} payload tokens, found {}", tokens.len());
    }
    Ok(())
}

fn strip_named<'a>(value: &'a str, name: &str) -> Result<&'a str> {
    value
        .strip_prefix(name)
        .with_context(|| format!("missing payload field {name}"))
}

fn parse_rss_stat(payload: &str) -> Result<ftrace_event::Event> {
    let tokens = payload.split_whitespace().collect::<Vec<_>>();
    exactly(&tokens, 4)?;
    let size = strip_named(tokens[3], "size=")?
        .strip_suffix('B')
        .context("missing size byte suffix")?;
    Ok(ftrace_event::Event::RssStatFormat(p::RssStatFormat {
        mm_id: parse_u32(strip_named(tokens[0], "mm_id=")?, "mm_id")?,
        curr: parse_u32(strip_named(tokens[1], "curr=")?, "curr")?,
        member: None,
        size: None,
        member_name: Some(strip_named(tokens[2], "type=")?.to_owned()),
        signed_size: Some(parse_i64(size, "size")?),
    }))
}

fn parse_cpu_idle(payload: &str) -> Result<ftrace_event::Event> {
    let tokens = payload.split_whitespace().collect::<Vec<_>>();
    exactly(&tokens, 2)?;
    Ok(ftrace_event::Event::CpuIdleFormat(p::CpuIdleFormat {
        state: parse_u32(strip_named(tokens[0], "state=")?, "state")?,
        cpu_id: parse_u32(strip_named(tokens[1], "cpu_id=")?, "cpu_id")?,
    }))
}

fn parse_irq_entry(payload: &str) -> Result<ftrace_event::Event> {
    let (irq, name) = take_between(payload, "irq=", " name=")?;
    Ok(ftrace_event::Event::IrqHandlerEntryFormat(
        p::IrqHandlerEntryFormat {
            irq: parse_i32(irq, "irq")?,
            name: name.to_owned(),
        },
    ))
}

fn parse_irq_exit(payload: &str) -> Result<ftrace_event::Event> {
    let (irq, result) = take_between(payload, "irq=", " ret=")?;
    Ok(ftrace_event::Event::IrqHandlerExitFormat(
        p::IrqHandlerExitFormat {
            irq: parse_i32(irq, "irq")?,
            ret: None,
            ret_symbol: Some(result.to_owned()),
        },
    ))
}

#[derive(Clone, Copy)]
enum SoftirqKind {
    Entry,
    Exit,
    Raise,
}

fn parse_softirq(payload: &str, kind: SoftirqKind) -> Result<ftrace_event::Event> {
    let (vector, action) = take_between(payload, "vec=", " [action=")?;
    let action = action.strip_suffix(']').context("missing action suffix")?;
    let vec = parse_u32(vector, "vec")?;
    let action = Some(action.to_owned());
    Ok(match kind {
        SoftirqKind::Entry => {
            ftrace_event::Event::SoftirqEntryFormat(p::SoftirqEntryFormat { vec, action })
        }
        SoftirqKind::Exit => {
            ftrace_event::Event::SoftirqExitFormat(p::SoftirqExitFormat { vec, action })
        }
        SoftirqKind::Raise => {
            ftrace_event::Event::SoftirqRaiseFormat(p::SoftirqRaiseFormat { vec, action })
        }
    })
}

fn parenthesized_reason(value: &str) -> Result<String> {
    Ok(value
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
        .context("reason must be parenthesized")?
        .to_owned())
}

fn parse_ipi_reason(payload: &str, exit: bool) -> Result<ftrace_event::Event> {
    let reason = parenthesized_reason(payload)?;
    Ok(if exit {
        ftrace_event::Event::IpiExitFormat(p::IpiExitFormat { reason })
    } else {
        ftrace_event::Event::IpiEntryFormat(p::IpiEntryFormat { reason })
    })
}

fn parse_ipi_raise(payload: &str) -> Result<ftrace_event::Event> {
    let (mask, reason) = take_between(payload, "target_mask=", " (")?;
    Ok(ftrace_event::Event::IpiRaiseFormat(p::IpiRaiseFormat {
        target_cpus: None,
        reason: parenthesized_reason(&format!("({reason}"))?,
        target_mask: Some(mask.to_owned()),
    }))
}

fn parse_ipi_send_cpu(payload: &str) -> Result<ftrace_event::Event> {
    let (cpu, rest) = take_between(payload, "cpu=", " callsite=")?;
    let (callsite, callback) = take_between(rest, "", " callback=")?;
    Ok(ftrace_event::Event::IpiSendCpuFormat(p::IpiSendCpuFormat {
        target_cpu: parse_u32(cpu, "cpu")?,
        callsite: callsite.to_owned(),
        callback: callback.to_owned(),
    }))
}

fn parse_workqueue(payload: &str, start: bool) -> Result<ftrace_event::Event> {
    let (work, function) = take_between(payload, "work struct ", ": function ")?;
    let work = parse_hex_u64(work, "work")?;
    let function_symbol = Some(function.to_owned());
    Ok(if start {
        ftrace_event::Event::WorkqueueExecuteStartFormat(p::WorkqueueExecuteStartFormat {
            work,
            function: None,
            function_symbol,
        })
    } else {
        ftrace_event::Event::WorkqueueExecuteEndFormat(p::WorkqueueExecuteEndFormat {
            work,
            function_symbol,
        })
    })
}

fn parse_binder_transaction(payload: &str) -> Result<ftrace_event::Event> {
    let tokens = payload.split_whitespace().collect::<Vec<_>>();
    exactly(&tokens, 7)?;
    Ok(ftrace_event::Event::BinderTransactionFormat(
        p::BinderTransactionFormat {
            debug_id: parse_i32(strip_named(tokens[0], "transaction=")?, "transaction")?,
            target_node: parse_i32(strip_named(tokens[1], "dest_node=")?, "dest_node")?,
            to_proc: parse_i32(strip_named(tokens[2], "dest_proc=")?, "dest_proc")?,
            to_thread: parse_i32(strip_named(tokens[3], "dest_thread=")?, "dest_thread")?,
            reply: parse_i32(strip_named(tokens[4], "reply=")?, "reply")?,
            flags: parse_hex_u32(strip_named(tokens[5], "flags=")?, "flags")?,
            code: parse_hex_u32(strip_named(tokens[6], "code=")?, "code")?,
        },
    ))
}

fn parse_binder_received(payload: &str) -> Result<ftrace_event::Event> {
    Ok(ftrace_event::Event::BinderTransactionReceivedFormat(
        p::BinderTransactionReceivedFormat {
            debug_id: parse_i32(strip_named(payload, "transaction=")?, "transaction")?,
        },
    ))
}

fn parse_block_bio_remap(payload: &str) -> Result<ftrace_event::Event> {
    let (current, old) = payload
        .split_once(" <- (")
        .context("missing remap separator")?;
    let tokens = current.split_whitespace().collect::<Vec<_>>();
    exactly(&tokens, 5)?;
    if tokens[3] != "+" {
        bail!("missing sector-count separator");
    }
    let (old_device, old_sector) = old.split_once(") ").context("invalid old device")?;
    let (major, minor) = parse_device(tokens[0])?;
    let (old_major, old_minor) = parse_device(old_device)?;
    Ok(ftrace_event::Event::BlockBioRemapFormat(
        p::BlockBioRemapFormat {
            dev: encode_device(major, minor),
            rwbs: tokens[1].to_owned(),
            sector: parse_u64(tokens[2], "sector")?,
            nr_sector: parse_u32(tokens[4], "sector_count")?,
            old_dev: encode_device(old_major, old_minor),
            old_sector: parse_u64(old_sector, "old_sector")?,
        },
    ))
}

fn take_parenthesized(value: &str) -> Result<(&str, &str)> {
    let value = value.strip_prefix('(').context("missing command prefix")?;
    value.split_once(") ").context("missing command suffix")
}

fn parse_block_complete(payload: &str) -> Result<ftrace_event::Event> {
    let (device, rest) = payload.split_once(' ').context("missing device")?;
    let (rwbs, rest) = rest.split_once(' ').context("missing rwbs")?;
    let (command, rest) = take_parenthesized(rest)?;
    let tokens = rest.split_whitespace().collect::<Vec<_>>();
    exactly(&tokens, 4)?;
    if tokens[1] != "+" {
        bail!("missing sector-count separator");
    }
    let error = tokens[3]
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .context("invalid completion error")?;
    let (major, minor) = parse_device(device)?;
    Ok(ftrace_event::Event::BlockRqCompleteFormat(
        p::BlockRqCompleteFormat {
            dev: encode_device(major, minor),
            rwbs: rwbs.to_owned(),
            cmd: command.to_owned(),
            sector: parse_u64(tokens[0], "sector")?,
            nr_sector: parse_u32(tokens[2], "sector_count")?,
            error: parse_i32(error, "error")?,
        },
    ))
}

fn parse_block_request(payload: &str, issue: bool) -> Result<ftrace_event::Event> {
    let (device, rest) = payload.split_once(' ').context("missing device")?;
    let (rwbs, rest) = rest.split_once(' ').context("missing rwbs")?;
    let (bytes, rest) = rest.split_once(' ').context("missing byte count")?;
    let (command, rest) = take_parenthesized(rest)?;
    let (range, comm) = rest.rsplit_once(" [").context("missing comm")?;
    let comm = comm.strip_suffix(']').context("missing comm suffix")?;
    let tokens = range.split_whitespace().collect::<Vec<_>>();
    exactly(&tokens, 3)?;
    if tokens[1] != "+" {
        bail!("missing sector-count separator");
    }
    let (major, minor) = parse_device(device)?;
    let dev = encode_device(major, minor);
    let sector = parse_u64(tokens[0], "sector")?;
    let nr_sector = parse_u32(tokens[2], "sector_count")?;
    let bytes = parse_u32(bytes, "bytes")?;
    let rwbs = rwbs.to_owned();
    let comm = comm.to_owned();
    let cmd = command.to_owned();
    Ok(if issue {
        ftrace_event::Event::BlockRqIssueFormat(p::BlockRqIssueFormat {
            dev,
            sector,
            nr_sector,
            bytes,
            rwbs,
            comm,
            cmd,
        })
    } else {
        ftrace_event::Event::BlockRqInsertFormat(p::BlockRqInsertFormat {
            dev,
            sector,
            nr_sector,
            bytes,
            rwbs,
            comm,
            cmd,
        })
    })
}

fn parse_kswapd_wake(payload: &str) -> Result<ftrace_event::Event> {
    let (node, order) = take_between(payload, "nid=", " order=")?;
    Ok(ftrace_event::Event::MmVmscanKswapdWakeFormat(
        p::MmVmscanKswapdWakeFormat {
            nid: parse_i32(node, "nid")?,
            zid: None,
            order: parse_i32(order, "order")?,
        },
    ))
}

fn parse_kswapd_sleep(payload: &str) -> Result<ftrace_event::Event> {
    Ok(ftrace_event::Event::MmVmscanKswapdSleepFormat(
        p::MmVmscanKswapdSleepFormat {
            nid: parse_i32(strip_named(payload, "nid=")?, "nid")?,
        },
    ))
}
