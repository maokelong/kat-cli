use anyhow::{Context, Result, bail};

use crate::proto::ftrace2parquet::{
    SchedSwitch, SchedWakeup, SchedWakeupNew, TextFtraceEvent, TracingMarkWrite,
    text_ftrace_event::Payload,
};

const TICKS_PER_SECOND: u64 = 1_000_000_000;

pub(crate) enum ParsedEvent {
    Supported(TextFtraceEvent),
    Unsupported(String),
}

pub(crate) fn parse_event(
    line: &str,
    clock_domain: &str,
    has_tgid_column: bool,
) -> Result<ParsedEvent> {
    let first_separator = line.find(": ").context("missing event separator")?;
    let cpu_start = line[..first_separator]
        .rfind(" [")
        .context("missing CPU field")?
        + 1;
    let cpu_end = line[cpu_start..].find("] ").context("invalid CPU field")? + cpu_start;
    let cpu = parse_u32(&line[cpu_start + 1..cpu_end], "CPU")?;
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
        _ => return Ok(ParsedEvent::Unsupported(event_name.to_owned())),
    };
    Ok(ParsedEvent::Supported(TextFtraceEvent {
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

pub(crate) fn parse_u32(value: &str, label: &str) -> Result<u32> {
    value.parse().with_context(|| format!("invalid {label}"))
}
