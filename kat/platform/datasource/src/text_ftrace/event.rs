use anyhow::{Context, Result, bail};

use crate::proto::ftrace2parquet::{
    BinderTransaction, BlockRqComplete, BlockRqIssue, FilemapPageCache, Print, SchedBlockedReason,
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
        "sched_blocked_reason" => {
            Payload::SchedBlockedReason(parse_sched_blocked_reason(payload_text)?)
        }
        "mm_filemap_add_to_page_cache" => {
            Payload::MmFilemapAddToPageCache(parse_filemap_page_cache(payload_text)?)
        }
        "mm_filemap_delete_from_page_cache" => {
            Payload::MmFilemapDeleteFromPageCache(parse_filemap_page_cache(payload_text)?)
        }
        "block_rq_issue" => Payload::BlockRqIssue(parse_block_rq_issue(payload_text)?),
        "block_rq_complete" => Payload::BlockRqComplete(parse_block_rq_complete(payload_text)?),
        "binder_transaction" => Payload::BinderTransaction(parse_binder_transaction(payload_text)?),
        "print" => Payload::Print(parse_print(payload_text)?),
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

fn parse_sched_blocked_reason(payload: &str) -> Result<SchedBlockedReason> {
    let (pid, rest) = take_between(payload, "pid=", " iowait=")?;
    let (io_wait, caller) = take_between(rest, "", " caller=")?;
    Ok(SchedBlockedReason {
        pid: parse_i32(pid, "pid")?,
        io_wait: parse_u32(io_wait, "iowait")?,
        caller: caller.to_owned(),
    })
}

fn parse_filemap_page_cache(payload: &str) -> Result<FilemapPageCache> {
    let rest = payload.strip_prefix("dev ").context("missing dev field")?;
    let (device, rest) = rest.split_once(" ino ").context("missing ino field")?;
    let (device_major, device_minor) = parse_device(device)?;
    let (inode, fields) = rest
        .split_once(char::is_whitespace)
        .context("missing filemap fields")?;
    Ok(FilemapPageCache {
        device_major,
        device_minor,
        inode: parse_radix(inode, 16, "inode")?,
        page_frame_number: parse_integer(field(fields, "pfn")?, "pfn")?,
        offset_bytes: parse_u64(field(fields, "ofs")?, "ofs")?,
        order: optional_field(fields, "order")
            .map(|value| parse_u32(value, "order"))
            .transpose()?,
        page_address: optional_field(fields, "page").map(str::to_owned),
    })
}

fn parse_block_rq_issue(payload: &str) -> Result<BlockRqIssue> {
    let (device, rest) = payload.split_once(' ').context("missing block rwbs")?;
    let (device_major, device_minor) = parse_device(device)?;
    let (rwbs, rest) = rest.split_once(' ').context("missing block bytes")?;
    let (bytes, rest) = rest.split_once(' ').context("missing block command")?;
    let (command, sector, sector_count, process_name) = parse_block_tail(rest, "process")?;
    Ok(BlockRqIssue {
        device_major,
        device_minor,
        rwbs: rwbs.to_owned(),
        bytes: parse_u32(bytes, "bytes")?,
        command,
        sector,
        sector_count,
        process_name,
    })
}

fn parse_block_rq_complete(payload: &str) -> Result<BlockRqComplete> {
    let (device, rest) = payload.split_once(' ').context("missing block rwbs")?;
    let (device_major, device_minor) = parse_device(device)?;
    let (rwbs, rest) = rest.split_once(' ').context("missing block command")?;
    let (command, sector, sector_count, error) = parse_block_tail(rest, "error")?;
    Ok(BlockRqComplete {
        device_major,
        device_minor,
        rwbs: rwbs.to_owned(),
        command,
        sector,
        sector_count,
        error: parse_i32(&error, "error")?,
    })
}

fn parse_block_tail(payload: &str, final_label: &str) -> Result<(String, u64, u32, String)> {
    let rest = payload
        .strip_prefix('(')
        .context("missing block command opening delimiter")?;
    let (command, rest) = rest
        .split_once(") ")
        .context("missing block command closing delimiter")?;
    let (sector, rest) = rest.split_once(" + ").context("missing sector count")?;
    let (sector_count, final_value) = rest
        .split_once(" [")
        .with_context(|| format!("missing block {final_label}"))?;
    let final_value = final_value
        .strip_suffix(']')
        .with_context(|| format!("missing block {final_label} closing delimiter"))?;
    Ok((
        command.to_owned(),
        parse_u64(sector, "sector")?,
        parse_u32(sector_count, "sector count")?,
        final_value.to_owned(),
    ))
}

fn parse_binder_transaction(payload: &str) -> Result<BinderTransaction> {
    let (transaction_id, rest) = take_between(payload, "transaction=", " dest_node=")?;
    let (destination_node_id, rest) = take_between(rest, "", " dest_proc=")?;
    let (destination_process_id, rest) = take_between(rest, "", " dest_thread=")?;
    let (destination_thread_id, rest) = take_between(rest, "", " reply=")?;
    let (reply, rest) = take_between(rest, "", " flags=")?;
    let (flags, code) = take_between(rest, "", " code=")?;
    Ok(BinderTransaction {
        transaction_id: parse_i32(transaction_id, "transaction")?,
        destination_node_id: parse_i32(destination_node_id, "dest_node")?,
        destination_process_id: parse_i32(destination_process_id, "dest_proc")?,
        destination_thread_id: parse_i32(destination_thread_id, "dest_thread")?,
        reply: parse_i32(reply, "reply")?,
        flags: parse_integer(flags, "flags")?,
        code: parse_integer(code, "code")?,
    })
}

fn parse_print(payload: &str) -> Result<Print> {
    let (instruction_pointer, content) = payload
        .split_once(": ")
        .context("missing print instruction pointer")?;
    Ok(Print {
        instruction_pointer: instruction_pointer.to_owned(),
        content: content.to_owned(),
    })
}

fn parse_device(value: &str) -> Result<(u32, u32)> {
    let (major, minor) = value.split_once([',', ':']).context("invalid device")?;
    Ok((
        parse_u32(major, "device major")?,
        parse_u32(minor, "device minor")?,
    ))
}

fn field<'a>(fields: &'a str, name: &str) -> Result<&'a str> {
    optional_field(fields, name).with_context(|| format!("missing {name} field"))
}

fn optional_field<'a>(fields: &'a str, name: &str) -> Option<&'a str> {
    fields.split_whitespace().find_map(|candidate| {
        let (candidate_name, value) = candidate.split_once('=')?;
        (candidate_name == name).then_some(value)
    })
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

fn parse_u64(value: &str, label: &str) -> Result<u64> {
    value.parse().with_context(|| format!("invalid {label}"))
}

fn parse_integer<T>(value: &str, label: &str) -> Result<T>
where
    T: TryFrom<u64>,
{
    let parsed = if let Some(hex) = value.strip_prefix("0x") {
        parse_radix(hex, 16, label)?
    } else {
        parse_u64(value, label)?
    };
    T::try_from(parsed).map_err(|_| anyhow::anyhow!("{label} overflows"))
}

fn parse_radix(value: &str, radix: u32, label: &str) -> Result<u64> {
    u64::from_str_radix(value, radix).with_context(|| format!("invalid {label}"))
}
