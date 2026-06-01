use crate::parser::{HarmonyTraceParser, ParseResult};
use crate::plugins::shared::{handle_trace_marker, parse_trace_marker, SharedTraceState};
use htrace_core::TraceEngineError;
use htrace_model::{
    CallstackRow, IrqRow, ParsedTrace, ProcessRow, RawEventRow, SchedSliceRow, ThreadRow,
    ThreadStateRow, TraceTableBuilder,
};
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

const ARG_DATATYPE_INT: u32 = 0;
const ARG_DATATYPE_STRING: u32 = 1;
const ARG_DATATYPE_BOOLEAN: u32 = 3;
const BINDER_ONEWAY_FLAG: i64 = 0x01;
const BINDER_ROOT_OBJECT_FLAG: i64 = 0x04;
const BINDER_STATUS_CODE_FLAG: i64 = 0x08;
const BINDER_ACCEPT_FDS_FLAG: i64 = 0x10;

#[derive(Debug, Clone)]
struct BytraceLine {
    task: Option<String>,
    pid: u32,
    tgid: Option<u32>,
    cpu: u32,
    flags: String,
    ts: i64,
    event_name: String,
    args: String,
}

#[derive(Debug, Clone)]
struct SchedSwitchEvent {
    prev_comm: String,
    prev_pid: u32,
    prev_state: String,
    next_comm: String,
    next_pid: u32,
    next_prio: i32,
}

#[derive(Debug, Clone)]
struct ThreadInfo {
    utid: u32,
    tid: u32,
    upid: u32,
    name: Option<String>,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
}

#[derive(Debug, Clone)]
struct OpenSchedSlice {
    row_id: usize,
    ts: i64,
}

#[derive(Default)]
struct BinderTextState {
    sync_transaction_by_id: HashMap<i64, PendingBinderTransaction>,
    reply_by_tid: HashMap<u32, usize>,
    reply_destination_by_tid: HashMap<u32, u32>,
    reply_waiting_by_id: HashSet<i64>,
}

#[derive(Debug, Clone, Copy)]
struct PendingBinderTransaction {
    row_id: usize,
    sender_tid: u32,
}

#[derive(Default)]
pub struct BytraceParser {
    tables: TraceTableBuilder,
    threads_by_tid: BTreeMap<u32, ThreadInfo>,
    cpu_running: HashMap<u32, OpenSchedSlice>,
    thread_state_open: HashMap<u32, usize>,
    pending_wakeup_by_tid: HashMap<u32, i64>,
    open_softirqs: HashMap<(u32, u32), usize>,
    shared_trace: SharedTraceState,
    binder_state: BinderTextState,
    next_id: u32,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    clock_domain: String,
    input_hash: u64,
}

impl BytraceParser {
    pub fn new() -> Self {
        Self {
            clock_domain: "unknown".to_string(),
            ..Self::default()
        }
    }

    fn reset_for_input(&mut self, bytes: &[u8]) {
        *self = Self::new();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        self.input_hash = hasher.finish();
        self.tables
            .push_metadata("parser", Some("htrace-parser-harmony"));
        self.tables.push_metadata("parser_version", Some("0.1.0"));
        self.tables
            .push_metadata("source_format", Some("bytrace_text"));
    }

    fn parse_text(&mut self, text: &str) -> ParseResult<()> {
        for (line_index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            match parse_bytrace_line(line) {
                Some(event) => self.handle_event(event)?,
                None => self.tables.push_raw_event(RawEventRow {
                    ts: None,
                    cpu: None,
                    tid: None,
                    event_name: "malformed_bytrace_line".to_string(),
                    payload_json: Some(
                        json!({
                            "line": line_index + 1,
                            "text": trimmed
                        })
                        .to_string(),
                    ),
                }),
            }
        }
        Ok(())
    }

    fn handle_event(&mut self, event: BytraceLine) -> ParseResult<()> {
        self.observe_ts(event.ts);

        match event.event_name.as_str() {
            "sched_switch" => match parse_sched_switch(&event.args) {
                Some(sched_switch) => self.on_sched_switch(event.ts, event.cpu, sched_switch),
                None => {
                    self.push_raw_event(&event, "malformed_sched_switch");
                    Ok(())
                }
            },
            "sched_wakeup" | "sched_wakeup_new" => {
                if let Some(wakeup) = parse_wakeup(&event.args) {
                    self.pending_wakeup_by_tid
                        .entry(wakeup.pid)
                        .or_insert(event.ts);
                }
                self.push_raw_event(&event, event.event_name.as_str());
                Ok(())
            }
            "tracing_mark_write" | "print" => {
                if let Some(marker) = parse_trace_marker(&event.args) {
                    handle_trace_marker(
                        &mut self.tables,
                        &mut self.shared_trace,
                        event.ts,
                        event.pid,
                        marker,
                    );
                }
                self.push_raw_event(&event, event.event_name.as_str());
                Ok(())
            }
            "binder_transaction" => {
                self.on_binder_transaction(&event);
                self.push_raw_event(&event, event.event_name.as_str());
                Ok(())
            }
            "binder_transaction_received" => {
                self.on_binder_transaction_received(&event);
                self.push_raw_event(&event, event.event_name.as_str());
                Ok(())
            }
            "binder_transaction_alloc_buf" => {
                self.on_binder_transaction_alloc_buf(&event);
                self.push_raw_event(&event, event.event_name.as_str());
                Ok(())
            }
            "softirq_entry" => {
                self.on_softirq_entry(&event);
                self.push_raw_event(&event, event.event_name.as_str());
                Ok(())
            }
            "softirq_exit" => {
                self.on_softirq_exit(&event);
                self.push_raw_event(&event, event.event_name.as_str());
                Ok(())
            }
            _ => {
                self.push_raw_event(&event, event.event_name.as_str());
                Ok(())
            }
        }
    }

    fn on_binder_transaction(&mut self, event: &BytraceLine) {
        let Some(transaction_id) = parse_i64_arg(&event.args, "transaction") else {
            return;
        };
        let reply = parse_i64_arg(&event.args, "reply").unwrap_or_default() == 1;
        if reply {
            if let Some(row_id) = self.binder_state.reply_by_tid.remove(&event.pid) {
                if let Some(dest_tid) = parse_i64_arg(&event.args, "dest_thread")
                    .and_then(|tid| u32::try_from(tid).ok())
                {
                    if self
                        .binder_state
                        .reply_destination_by_tid
                        .get(&event.pid)
                        .copied()
                        == Some(dest_tid)
                    {
                        let dest_name = self.thread_name_for_tid(dest_tid);
                        self.append_destination_thread_args(row_id, dest_tid, dest_name.as_deref());
                        self.binder_state
                            .reply_destination_by_tid
                            .remove(&event.pid);
                    }
                }
                let argset = self.ensure_callstack_argset(row_id);
                self.append_binder_transaction_args(argset, event);
                self.close_callstack_row(row_id, event.ts);
            }
            self.binder_state.reply_waiting_by_id.insert(transaction_id);
            return;
        }

        let argset = self.binder_argset(event);
        let row_id = self.push_binder_row(
            event.ts,
            event.pid,
            event.task.as_deref(),
            "binder transaction",
            None,
            Some(argset),
        );
        self.binder_state.sync_transaction_by_id.insert(
            transaction_id,
            PendingBinderTransaction {
                row_id,
                sender_tid: event.pid,
            },
        );
    }

    fn on_binder_transaction_received(&mut self, event: &BytraceLine) {
        let Some(transaction_id) = parse_i64_arg(&event.args, "transaction") else {
            return;
        };
        if let Some(pending) = self
            .binder_state
            .sync_transaction_by_id
            .remove(&transaction_id)
        {
            self.close_callstack_row(pending.row_id, event.ts);
            let row_id = self.push_binder_row(
                event.ts,
                event.pid,
                event.task.as_deref(),
                "binder reply",
                None,
                None,
            );
            let reply_slice_id = self.tables.callstack_id_at(row_id).unwrap_or_default() as i64;
            let trans_slice_id = self
                .tables
                .callstack_id_at(pending.row_id)
                .unwrap_or_default() as i64;
            let dest_name = self.thread_name_for_tid(event.pid);
            self.append_destination_thread_args(pending.row_id, event.pid, dest_name.as_deref());
            self.append_int_arg_to_callstack(
                pending.row_id,
                "destination slice id",
                reply_slice_id,
            );
            self.append_int_arg_to_callstack(row_id, "destination slice id", trans_slice_id);
            self.binder_state.reply_by_tid.insert(event.pid, row_id);
            self.binder_state
                .reply_destination_by_tid
                .insert(event.pid, pending.sender_tid);
            return;
        }
        if self
            .binder_state
            .reply_waiting_by_id
            .remove(&transaction_id)
        {
            return;
        }
        let row_id = self.push_binder_row(
            event.ts,
            event.pid,
            event.task.as_deref(),
            "binder reply",
            None,
            None,
        );
        self.binder_state.reply_by_tid.insert(event.pid, row_id);
    }

    fn on_binder_transaction_alloc_buf(&mut self, event: &BytraceLine) {
        let Some(transaction_id) = parse_i64_arg(&event.args, "transaction") else {
            return;
        };
        let Some(pending) = self
            .binder_state
            .sync_transaction_by_id
            .get(&transaction_id)
            .copied()
        else {
            return;
        };
        if let Some(data_size) = parse_i64_arg(&event.args, "data_size") {
            self.append_int_arg_to_callstack(pending.row_id, "data size", data_size);
        }
        if let Some(offsets_size) = parse_i64_arg(&event.args, "offsets_size") {
            self.append_int_arg_to_callstack(pending.row_id, "offsets size", offsets_size);
        }
    }

    fn push_binder_row(
        &mut self,
        ts: i64,
        tid: u32,
        name_hint: Option<&str>,
        name: &str,
        dur: Option<i64>,
        argsetid: Option<u64>,
    ) -> usize {
        let utid = self.get_or_create_thread(ts, tid, name_hint);
        self.tables.push_callstack(CallstackRow {
            id: self.tables.next_callstack_id(),
            ts,
            dur,
            callid: Some(utid),
            cat: Some("binder".to_string()),
            name: Some(name.to_string()),
            depth: Some(0),
            cookie: None,
            parent_id: None,
            argsetid,
            chain_id: None,
            span_id: None,
            parent_span_id: None,
            flag: None,
            trace_level: None,
            trace_tag: None,
            custom_category: None,
            custom_args: None,
            child_callid: None,
        })
    }

    fn close_callstack_row(&mut self, row_id: usize, ts: i64) {
        if let Some(row) = self.tables.callstack_mut(row_id) {
            row.dur = Some(ts.saturating_sub(row.ts));
        }
    }

    fn binder_argset(&mut self, event: &BytraceLine) -> u64 {
        let argset = self.tables.next_argset_id();
        self.append_binder_transaction_args(argset, event);
        argset
    }

    fn append_binder_transaction_args(&mut self, argset: u64, event: &BytraceLine) {
        self.push_int_arg(
            argset,
            "transaction id",
            parse_i64_arg(&event.args, "transaction").unwrap_or_default(),
        );
        self.push_int_arg(
            argset,
            "destination node",
            parse_i64_arg(&event.args, "dest_node").unwrap_or_default(),
        );
        self.push_int_arg(
            argset,
            "destination process",
            parse_i64_arg(&event.args, "dest_proc").unwrap_or_default(),
        );
        self.push_bool_arg(
            argset,
            "reply transaction?",
            parse_i64_arg(&event.args, "reply").unwrap_or_default() == 1,
        );
        self.push_string_arg(
            argset,
            "flags",
            &format_binder_flags(parse_i64_arg(&event.args, "flags").unwrap_or_default()),
        );
        self.push_string_arg(
            argset,
            "code",
            &format_binder_code(parse_i64_arg(&event.args, "code").unwrap_or_default()),
        );
        self.push_int_arg(argset, "calling tid", i64::from(event.pid));
    }

    fn append_int_arg_to_callstack(&mut self, row_id: usize, key: &str, value: i64) {
        let argset = self.ensure_callstack_argset(row_id);
        self.push_int_arg(argset, key, value);
    }

    fn append_string_arg_to_callstack(&mut self, row_id: usize, key: &str, value: &str) {
        let argset = self.ensure_callstack_argset(row_id);
        self.push_string_arg(argset, key, value);
    }

    fn append_destination_thread_args(
        &mut self,
        row_id: usize,
        destination_tid: u32,
        destination_name: Option<&str>,
    ) {
        self.append_int_arg_to_callstack(row_id, "destination thread", i64::from(destination_tid));
        self.append_string_arg_to_callstack(
            row_id,
            "destination name",
            destination_name.unwrap_or(""),
        );
    }

    fn ensure_callstack_argset(&mut self, row_id: usize) -> u64 {
        let existing_argset = self
            .tables
            .callstack_mut(row_id)
            .and_then(|row| row.argsetid);
        let argset = existing_argset.unwrap_or_else(|| self.tables.next_argset_id());
        if existing_argset.is_none() {
            if let Some(row) = self.tables.callstack_mut(row_id) {
                row.argsetid = Some(argset);
            }
        }
        argset
    }

    fn push_int_arg(&mut self, argset: u64, key: &str, value: i64) {
        let key_id = self.tables.intern_string(key);
        self.tables
            .push_arg(key_id, ARG_DATATYPE_INT, value, argset);
    }

    fn push_bool_arg(&mut self, argset: u64, key: &str, value: bool) {
        let key_id = self.tables.intern_string(key);
        self.tables.push_arg(
            key_id,
            ARG_DATATYPE_BOOLEAN,
            if value { 1 } else { 0 },
            argset,
        );
    }

    fn push_string_arg(&mut self, argset: u64, key: &str, value: &str) {
        let key_id = self.tables.intern_string(key);
        let value_id = self.tables.intern_string(value);
        self.tables
            .push_arg(key_id, ARG_DATATYPE_STRING, value_id as i64, argset);
    }

    fn on_softirq_entry(&mut self, event: &BytraceLine) {
        let Some(vec) = parse_i64_arg(&event.args, "vec").and_then(|vec| u32::try_from(vec).ok())
        else {
            return;
        };
        let id = self.tables.next_irq_id();
        let row_id = self.tables.push_irq(IrqRow {
            id,
            ts: event.ts,
            dur: None,
            callid: Some(vec as i32),
            cat: "softirq".to_string(),
            name: parse_action(&event.args)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| softirq_name(vec).to_string()),
            depth: Some(0),
            cookie: None,
            parent_id: None,
            argsetid: None,
            flag: Some("1".to_string()),
        });
        self.open_softirqs.insert((event.cpu, vec), row_id);
    }

    fn on_softirq_exit(&mut self, event: &BytraceLine) {
        let Some(vec) = parse_i64_arg(&event.args, "vec").and_then(|vec| u32::try_from(vec).ok())
        else {
            return;
        };
        let Some(row_id) = self.open_softirqs.remove(&(event.cpu, vec)) else {
            return;
        };
        if let Some(row) = self.tables.irq_mut(row_id) {
            row.dur = Some(event.ts.saturating_sub(row.ts));
        }
        let argset = self.tables.next_argset_id();
        if let Some(row) = self.tables.irq_mut(row_id) {
            row.argsetid = Some(argset);
        }
        self.push_string_arg(
            argset,
            "irq_ret",
            parse_action(&event.args).unwrap_or(softirq_name(vec)),
        );
        self.push_int_arg(argset, "vec", i64::from(vec));
    }

    fn push_raw_event(&mut self, event: &BytraceLine, event_name: &str) {
        self.tables.push_raw_event(RawEventRow {
            ts: Some(event.ts),
            cpu: Some(event.cpu),
            tid: Some(event.pid),
            event_name: event_name.to_string(),
            payload_json: Some(event_payload(event).to_string()),
        });
    }

    fn on_sched_switch(&mut self, ts: i64, cpu: u32, event: SchedSwitchEvent) -> ParseResult<()> {
        let prev_utid =
            self.get_or_create_thread(ts, event.prev_pid, Some(event.prev_comm.as_str()));
        let next_utid =
            self.get_or_create_thread(ts, event.next_pid, Some(event.next_comm.as_str()));

        if let Some(open) = self.cpu_running.remove(&cpu) {
            if let Some(row) = self.tables.sched_slice_mut(open.row_id) {
                row.dur = Some(ts.saturating_sub(open.ts));
                row.end_state = Some(event.prev_state.clone());
            }
        }

        let row_id = self.tables.push_sched_slice(SchedSliceRow {
            cpu,
            utid: next_utid,
            ts,
            dur: None,
            priority: Some(event.next_prio),
            end_state: None,
        });
        self.cpu_running.insert(cpu, OpenSchedSlice { row_id, ts });

        if event.prev_pid != 0 {
            self.transition_thread_state(prev_utid, ts, event.prev_state, None);
        }
        if event.next_pid != 0 {
            if let Some(wakeup_ts) = self.pending_wakeup_by_tid.remove(&event.next_pid) {
                self.transition_thread_state(next_utid, wakeup_ts, "runnable".to_string(), None);
            }
            self.transition_thread_state(next_utid, ts, "running".to_string(), None);
        }
        Ok(())
    }

    fn transition_thread_state(
        &mut self,
        utid: u32,
        ts: i64,
        state: String,
        waker_utid: Option<u32>,
    ) {
        if let Some(row_id) = self.thread_state_open.remove(&utid) {
            if let Some(row) = self.tables.thread_state_mut(row_id) {
                row.dur = Some(ts.saturating_sub(row.ts));
            }
        }

        let row_id = self.tables.push_thread_state(ThreadStateRow {
            utid,
            ts,
            dur: None,
            state,
            io_wait: None,
            blocked_function: None,
            waker_utid,
        });
        self.thread_state_open.insert(utid, row_id);
    }

    fn get_or_create_thread(&mut self, ts: i64, tid: u32, name: Option<&str>) -> u32 {
        if let Some(info) = self.threads_by_tid.get_mut(&tid) {
            if let Some(name) = valid_thread_name(name) {
                info.name = Some(name.to_string());
            }
            info.end_ts = Some(ts);
            return info.utid;
        }

        let id = self.next_id;
        self.next_id += 1;
        let name = valid_thread_name(name)
            .map(ToOwned::to_owned)
            .or_else(|| (tid == 0).then(|| "idle".to_string()));

        self.threads_by_tid.insert(
            tid,
            ThreadInfo {
                utid: id,
                tid,
                upid: id,
                name,
                start_ts: Some(ts),
                end_ts: Some(ts),
            },
        );
        id
    }

    fn thread_name_for_tid(&self, tid: u32) -> Option<String> {
        self.threads_by_tid
            .get(&tid)
            .and_then(|info| valid_thread_name(info.name.as_deref()))
            .map(ToOwned::to_owned)
    }

    fn observe_ts(&mut self, ts: i64) {
        self.start_ts = Some(self.start_ts.map_or(ts, |current| current.min(ts)));
        self.end_ts = Some(self.end_ts.map_or(ts, |current| current.max(ts)));
    }

    fn finish_open_intervals(&mut self) {
        // Match TraceStreamer: intervals still open at EOF keep NULL duration.
    }

    fn finish(mut self) -> ParseResult<ParsedTrace> {
        self.finish_open_intervals();

        for info in self.threads_by_tid.values() {
            self.tables.push_process(ProcessRow {
                upid: info.upid,
                pid: info.tid,
                name: info.name.clone(),
                start_ts: info.start_ts,
                end_ts: info.end_ts,
            });
            self.tables.push_thread(ThreadRow {
                utid: info.utid,
                tid: info.tid,
                upid: info.upid,
                name: info.name.clone(),
                is_main: true,
            });
        }

        let trace_id = format!("bytrace:{:016x}", self.input_hash);
        let tables = self
            .tables
            .finish(
                trace_id.clone(),
                self.start_ts,
                self.end_ts,
                self.clock_domain.clone(),
            )
            .map_err(|err| {
                TraceEngineError::Engine(format!("failed to build Arrow tables: {err}"))
            })?;

        Ok(ParsedTrace {
            trace_id,
            start_ts: self.start_ts,
            end_ts: self.end_ts,
            clock_domain: self.clock_domain,
            tables,
        })
    }
}

impl HarmonyTraceParser for BytraceParser {
    fn parse_file(&mut self, path: &Path) -> ParseResult<ParsedTrace> {
        let bytes = fs::read(path)?;
        self.parse_bytes(&bytes)
    }

    fn parse_bytes(&mut self, bytes: &[u8]) -> ParseResult<ParsedTrace> {
        self.reset_for_input(bytes);
        let text = String::from_utf8_lossy(bytes);
        self.parse_text(&text)?;
        let parser = std::mem::take(self);
        parser.finish()
    }
}

pub(crate) fn looks_like_bytrace_text(bytes: &[u8]) -> bool {
    let sample_len = bytes.len().min(64 * 1024);
    if sample_len == 0 {
        return false;
    }

    let text = String::from_utf8_lossy(&bytes[..sample_len]);
    text.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty() && !trimmed.starts_with('#') && parse_bytrace_line(line).is_some()
    })
}

fn parse_bytrace_line(line: &str) -> Option<BytraceLine> {
    let close_cpu = line.find(']')?;
    let open_cpu = line[..close_cpu].rfind('[')?;
    let cpu = line[open_cpu + 1..close_cpu].trim().parse::<u32>().ok()?;
    let (task, pid, tgid) = parse_prefix(&line[..open_cpu])?;

    let after_cpu = line[close_cpu + 1..].trim_start();
    let (flags, rest) = take_token(after_cpu)?;
    let (ts_token, rest) = take_token(rest.trim_start())?;
    let ts = parse_seconds_to_ns(ts_token.strip_suffix(':')?)?;
    let (event_token, rest) = take_token(rest.trim_start())?;
    let event_name = event_token.strip_suffix(':')?.to_string();

    Some(BytraceLine {
        task,
        pid,
        tgid,
        cpu,
        flags: flags.to_string(),
        ts,
        event_name,
        args: rest.trim_start().to_string(),
    })
}

fn parse_prefix(prefix: &str) -> Option<(Option<String>, u32, Option<u32>)> {
    let prefix = prefix.trim_end();
    let close_tgid = prefix.rfind(')')?;
    let open_tgid = prefix[..close_tgid].rfind('(')?;
    let tgid = parse_optional_u32(&prefix[open_tgid + 1..close_tgid]);
    let task_pid = prefix[..open_tgid].trim_end();
    let (task, pid) = task_pid.rsplit_once('-')?;
    let task = task.trim();
    let task = (!task.is_empty()).then(|| task.to_string());
    let pid = pid.trim().parse::<u32>().ok()?;
    Some((task, pid, tgid))
}

fn parse_optional_u32(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.is_empty() || value.chars().all(|ch| ch == '-') {
        return None;
    }
    value.parse::<u32>().ok()
}

fn take_token(input: &str) -> Option<(&str, &str)> {
    let input = input.trim_start();
    if input.is_empty() {
        return None;
    }
    match input.find(char::is_whitespace) {
        Some(index) => Some((&input[..index], &input[index..])),
        None => Some((input, "")),
    }
}

fn parse_seconds_to_ns(value: &str) -> Option<i64> {
    let (seconds, fraction) = value.split_once('.')?;
    let seconds = seconds.parse::<i64>().ok()?;
    let mut nanos = 0i64;
    let mut place = 100_000_000i64;
    for ch in fraction.chars().take(9) {
        let digit = ch.to_digit(10)? as i64;
        nanos += digit * place;
        place /= 10;
    }
    seconds.checked_mul(1_000_000_000)?.checked_add(nanos)
}

fn parse_sched_switch(args: &str) -> Option<SchedSwitchEvent> {
    let args = parse_key_values(args);
    let _prev_prio = args.get("prev_prio")?.parse::<i32>().ok()?;
    Some(SchedSwitchEvent {
        prev_comm: args.get("prev_comm")?.to_string(),
        prev_pid: args.get("prev_pid")?.parse::<u32>().ok()?,
        prev_state: state_from_bytrace(args.get("prev_state")?),
        next_comm: args.get("next_comm")?.to_string(),
        next_pid: args.get("next_pid")?.parse::<u32>().ok()?,
        next_prio: args.get("next_prio")?.parse::<i32>().ok()?,
    })
}

#[derive(Debug, Clone, Copy)]
struct WakeupEvent {
    pid: u32,
}

fn parse_wakeup(args: &str) -> Option<WakeupEvent> {
    let args = parse_key_values(args);
    Some(WakeupEvent {
        pid: args.get("pid")?.parse::<u32>().ok()?,
    })
}

fn parse_key_values(args: &str) -> HashMap<String, String> {
    args.split_whitespace()
        .filter(|token| *token != "==>")
        .filter_map(|token| token.split_once('='))
        .map(|(key, value)| (key.to_string(), value.trim_end_matches(',').to_string()))
        .collect()
}

fn parse_raw_arg<'a>(args: &'a str, key: &str) -> Option<&'a str> {
    args.split_whitespace()
        .filter_map(|token| token.split_once('='))
        .find_map(|(candidate, value)| (candidate == key).then_some(value.trim_end_matches(',')))
}

fn parse_i64_arg(args: &str, key: &str) -> Option<i64> {
    let value = parse_raw_arg(args, key)?;
    if let Some(hex) = value.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).ok()
    } else {
        value.parse().ok()
    }
}

fn parse_action(args: &str) -> Option<&str> {
    let start = args.find("[action=")? + "[action=".len();
    let rest = &args[start..];
    let end = rest.find(']')?;
    Some(&rest[..end])
}

fn valid_thread_name(name: Option<&str>) -> Option<&str> {
    name.filter(|value| !value.is_empty() && *value != "<...>")
}

fn format_binder_flags(flags: i64) -> String {
    format!("0x{:x}{}", flags, binder_flags_desc(flags).trim_end())
}

fn format_binder_code(code: i64) -> String {
    format!("0x{:x} Java Layer Dependent", code)
}

fn binder_flags_desc(flags: i64) -> String {
    let mut desc = String::new();
    if (flags & BINDER_ONEWAY_FLAG) == BINDER_ONEWAY_FLAG {
        desc.push_str(" this is a one-way call: async, no return; ");
    }
    if (flags & BINDER_ROOT_OBJECT_FLAG) == BINDER_ROOT_OBJECT_FLAG {
        desc.push_str(" contents are the components root object; ");
    }
    if (flags & BINDER_STATUS_CODE_FLAG) == BINDER_STATUS_CODE_FLAG {
        desc.push_str(" contents are a 32-bit status code; ");
    }
    if (flags & BINDER_ACCEPT_FDS_FLAG) == BINDER_ACCEPT_FDS_FLAG {
        desc.push_str(" allow replies with file descriptors; ");
    }
    if flags == 0 {
        desc.push_str(" No Flags Set");
    }
    desc
}

fn softirq_name(vec: u32) -> &'static str {
    match vec {
        0 => "HI",
        1 => "TIMER",
        2 => "NET_TX",
        3 => "NET_RX",
        4 => "BLOCK",
        5 => "IRQ_POLL",
        6 => "TASKLET",
        7 => "SCHED",
        8 => "HRTIMER",
        9 => "RCU",
        _ => "UNKNOWN",
    }
}

fn state_from_bytrace(raw: &str) -> String {
    if raw.contains('R') {
        return "runnable".to_string();
    }

    match raw.chars().next() {
        Some('S') => "sleeping".to_string(),
        Some('D') => "uninterruptible".to_string(),
        Some('T') => "stopped".to_string(),
        Some('t') => "traced".to_string(),
        Some('X') | Some('Z') => "exit".to_string(),
        Some('P') => "parked".to_string(),
        Some('I') => "dead".to_string(),
        Some(_) => format!("state_{raw}"),
        None => "unknown".to_string(),
    }
}

fn event_payload(event: &BytraceLine) -> serde_json::Value {
    json!({
        "task": event.task,
        "tgid": event.tgid,
        "flags": event.flags,
        "args": event.args
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_single_sched_switch_line() {
        let line = "          <idle>-0     (-----) [001] d..3 168758.663017: sched_switch: prev_comm=swapper/1 prev_pid=0 prev_prio=120 prev_state=R ==> next_comm=Binder:924_3 next_pid=1200 next_prio=120";

        let parsed = BytraceParser::default()
            .parse_bytes(line.as_bytes())
            .expect("parse bytrace line");

        assert_eq!(parsed.trace_id.split(':').next(), Some("bytrace"));
        assert_eq!(parsed.start_ts, Some(168_758_663_017_000));
        assert_eq!(parsed.tables.sched_slice.num_rows(), 1);
        assert_eq!(parsed.tables.thread_state.num_rows(), 1);
        assert_eq!(parsed.tables.thread.num_rows(), 2);
    }

    #[test]
    fn keeps_malformed_lines_in_raw_event() {
        let parsed = BytraceParser::default()
            .parse_bytes(b"this is not a bytrace row\n")
            .expect("parse malformed bytrace text");

        assert_eq!(parsed.tables.sched_slice.num_rows(), 0);
        assert_eq!(parsed.tables.raw_event.num_rows(), 1);
    }

    #[test]
    fn detects_bytrace_text() {
        let text = "# TRACE:\n          atrace-8528  ( 8528) [003] d..3 168758.663039: sched_switch: prev_comm=atrace prev_pid=8528 prev_prio=120 prev_state=S ==> next_comm=swapper/3 next_pid=0 next_prio=120\n";

        assert!(looks_like_bytrace_text(text.as_bytes()));
    }

    #[test]
    fn parses_repository_bytrace_fixture() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../test/resource/ut_bytrace_input_thread.txt");
        if !fixture.exists() {
            eprintln!("skip missing fixture {}", fixture.display());
            return;
        }

        let parsed = BytraceParser::default()
            .parse_file(&fixture)
            .expect("parse bytrace fixture");

        assert_eq!(parsed.tables.sched_slice.num_rows(), 15);
        assert!(parsed.tables.raw_event.num_rows() > 0);
        assert!(parsed.tables.thread.num_rows() > 0);
    }

    #[test]
    fn parses_trace_markers_into_shared_callstack_tables() {
        let text = "             app-42    (   42) [001] .... 1.000000: tracing_mark_write: B|42|Render##phase=prepare,count=2\n\
                    app-42    (   42) [001] .... 1.005000: tracing_mark_write: E\n\
                    app-42    (   42) [001] .... 1.006000: tracing_mark_write: C|42|fps|60\n";

        let parsed = BytraceParser::default()
            .parse_bytes(text.as_bytes())
            .expect("parse trace markers");

        assert_eq!(parsed.tables.callstack.num_rows(), 2);
        assert_eq!(parsed.tables.args.num_rows(), 3);
        assert_eq!(parsed.tables.data_dict.num_rows(), 8);
        assert_eq!(parsed.tables.raw_event.num_rows(), 3);
    }
}
