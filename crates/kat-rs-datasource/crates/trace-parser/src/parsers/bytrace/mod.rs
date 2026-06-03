use crate::plugins::shared;
use crate::{HarmonyTraceParser, ParseResult, TraceEngineError};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use trace_model::{
    CpuMeasureFilterRow, InstantRow, MeasureFilterRow, MeasureRow, ParsedTrace, ProcessRow,
    RawEventRow, RawRow, SchedSliceRow, ThreadRow, ThreadStateRow, TraceTableBuilder,
};

#[derive(Debug, Clone)]
struct ThreadInfo {
    utid: u32,
    tid: u32,
    upid: u32,
    name: Option<String>,
    end_ts: Option<i64>,
}

#[derive(Debug, Clone)]
struct ProcessInfoState {
    upid: u32,
    pid: u32,
    name: Option<String>,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
}

#[derive(Debug, Clone)]
struct OpenSchedSlice {
    row_id: usize,
    ts: i64,
}

struct TextTraceEvent<'a> {
    comm: &'a str,
    tid: u32,
    tgid: Option<u32>,
    cpu: u32,
    ts: i64,
    name: &'a str,
    payload: &'a str,
}

#[derive(Default)]
pub struct BytraceParser {
    tables: TraceTableBuilder,
    processes_by_pid: BTreeMap<u32, ProcessInfoState>,
    threads_by_tid: BTreeMap<u32, ThreadInfo>,
    cpu_running: HashMap<u32, OpenSchedSlice>,
    thread_state_open: HashMap<u32, usize>,
    pending_wakeup_by_tid: HashMap<u32, i64>,
    measure_filters: HashMap<(String, String, Option<u32>), u64>,
    open_measures: HashMap<u64, usize>,
    shared_trace: shared::SharedTraceState,
    next_id: u32,
    next_measure_filter_id: u64,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    input_hash: u64,
    parsed_events: usize,
    skipped_lines: usize,
}

impl BytraceParser {
    pub fn new() -> Self {
        Self::default()
    }

    fn reset_for_input(&mut self, bytes: &[u8]) {
        *self = Self::new();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        self.input_hash = hasher.finish();
        self.tables.push_metadata("parser", Some("trace-parser"));
        self.tables.push_metadata("parser_version", Some("0.1.0"));
        self.tables
            .push_metadata("source_format", Some("bytrace_text"));
    }

    fn parse_text(&mut self, bytes: &[u8]) -> ParseResult<()> {
        let text = std::str::from_utf8(bytes)
            .map_err(|err| TraceEngineError::Parse(format!("bytrace text is not utf-8: {err}")))?;
        for line in text.lines() {
            let Some(event) = parse_event_line(line) else {
                if !line.trim().is_empty() && !line.trim_start().starts_with('#') {
                    self.skipped_lines += 1;
                }
                continue;
            };
            self.parsed_events += 1;
            self.handle_event(event)?;
        }
        Ok(())
    }

    fn handle_event(&mut self, event: TextTraceEvent<'_>) -> ParseResult<()> {
        self.observe_ts(event.ts);
        self.get_or_create_thread(event.ts, event.tid, Some(event.comm), event.tgid);

        match event.name {
            "sched_switch" => self.on_sched_switch(event.ts, event.cpu, event.payload),
            "sched_wakeup" | "sched_wakeup_new" | "sched_waking" => {
                self.on_sched_wakeup_event(&event);
                Ok(())
            }
            "tracing_mark_write" => {
                self.on_trace_marker(&event);
                Ok(())
            }
            "cpu_idle" => {
                self.on_cpu_idle(&event);
                Ok(())
            }
            "cpu_frequency" => {
                self.on_cpu_frequency(&event);
                Ok(())
            }
            _ => {
                self.push_raw_event(&event);
                Ok(())
            }
        }
    }

    fn on_sched_switch(&mut self, ts: i64, cpu: u32, payload: &str) -> ParseResult<()> {
        let prev_comm = field_value(payload, "prev_comm").unwrap_or("");
        let prev_pid = field_value(payload, "prev_pid")
            .and_then(parse_u32)
            .unwrap_or(0);
        let prev_prio = field_value(payload, "prev_prio").and_then(parse_i32);
        let prev_state = field_value(payload, "prev_state").unwrap_or("R");
        let next_comm = field_value(payload, "next_comm").unwrap_or("");
        let next_pid = field_value(payload, "next_pid")
            .and_then(parse_u32)
            .unwrap_or(0);
        let next_prio = field_value(payload, "next_prio").and_then(parse_i32);

        let prev_utid = self.get_or_create_thread(ts, prev_pid, non_empty(prev_comm), None);
        let next_utid = self.get_or_create_thread(ts, next_pid, non_empty(next_comm), None);

        if let Some(open) = self.cpu_running.remove(&cpu) {
            if let Some(row) = self.tables.sched_slice_mut(open.row_id) {
                row.dur = Some(ts.saturating_sub(open.ts));
                row.end_state = Some(state_from_text(prev_state));
            }
        }

        let row_id = self.tables.push_sched_slice(SchedSliceRow {
            cpu,
            utid: next_utid,
            ts,
            dur: None,
            priority: next_prio,
            end_state: Some("runnable".to_string()),
        });
        self.cpu_running.insert(cpu, OpenSchedSlice { row_id, ts });

        if prev_pid != 0 {
            self.check_wakeup_event(prev_pid, prev_utid);
            self.transition_thread_state(prev_utid, ts, state_from_text(prev_state), None);
        }
        if next_pid != 0 {
            self.check_wakeup_event(next_pid, next_utid);
            self.transition_thread_state(next_utid, ts, "running".to_string(), None);
        }

        if prev_prio.is_none() {
            log::trace!(
                target: "trace_parser::bytrace",
                "sched_switch missing prev_prio payload={}",
                payload
            );
        }

        Ok(())
    }

    fn on_sched_wakeup_event(&mut self, event: &TextTraceEvent<'_>) {
        let target_tid = field_value(event.payload, "pid").and_then(parse_u32);
        let target_name = field_value(event.payload, "comm");
        let target_utid =
            target_tid.and_then(|tid| self.on_sched_wakeup(event.ts, tid, target_name));
        self.push_sched_instant(
            event.ts,
            event.cpu,
            Some(event.tid),
            event.name,
            target_utid,
        );
        self.tables.push_raw_event(RawEventRow {
            ts: Some(event.ts),
            cpu: Some(event.cpu),
            tid: Some(event.tid),
            event_name: event.name.to_string(),
            payload_json: Some(event_payload_json(event.payload).to_string()),
        });
    }

    fn on_trace_marker(&mut self, event: &TextTraceEvent<'_>) {
        let Some(marker) = shared::parse_trace_marker(event.payload) else {
            self.tables.push_raw_event(RawEventRow {
                ts: Some(event.ts),
                cpu: Some(event.cpu),
                tid: Some(event.tid),
                event_name: event.name.to_string(),
                payload_json: Some(json!({ "payload": event.payload }).to_string()),
            });
            return;
        };
        shared::handle_trace_marker(
            &mut self.tables,
            &mut self.shared_trace,
            event.ts,
            event.tgid.unwrap_or(event.tid),
            marker,
        );
    }

    fn on_cpu_idle(&mut self, event: &TextTraceEvent<'_>) {
        let state = field_value(event.payload, "state")
            .and_then(parse_u64)
            .unwrap_or(0);
        let cpu_id = field_value(event.payload, "cpu_id")
            .and_then(parse_u32)
            .unwrap_or(event.cpu);
        self.tables.push_raw(RawRow {
            id: self.tables.next_raw_id(),
            ts: event.ts,
            name: "cpu_idle".to_string(),
            cpu: event.cpu,
            itid: Some(0),
        });
        let filter_id = self.measure_filter("cpu_idle", "cpu_measure_filter", Some(cpu_id));
        self.push_measure(event.ts, filter_id, state as i64);
    }

    fn on_cpu_frequency(&mut self, event: &TextTraceEvent<'_>) {
        let state = field_value(event.payload, "state")
            .and_then(parse_u64)
            .unwrap_or(0);
        let cpu_id = field_value(event.payload, "cpu_id")
            .and_then(parse_u32)
            .unwrap_or(event.cpu);
        let filter_id = self.measure_filter("cpu_frequency", "cpu_measure_filter", Some(cpu_id));
        self.push_measure(event.ts, filter_id, state as i64);
    }

    fn push_raw_event(&mut self, event: &TextTraceEvent<'_>) {
        self.tables.push_raw_event(RawEventRow {
            ts: Some(event.ts),
            cpu: Some(event.cpu),
            tid: Some(event.tid),
            event_name: event.name.to_string(),
            payload_json: Some(event_payload_json(event.payload).to_string()),
        });
    }

    fn push_sched_instant(
        &mut self,
        ts: i64,
        cpu: u32,
        waker_tid: Option<u32>,
        event_name: &str,
        target_utid: Option<u32>,
    ) {
        let wakeup_from = waker_tid.map(|tid| self.get_or_create_thread(ts, tid, None, None));
        self.tables.push_raw(RawRow {
            id: self.tables.next_raw_id(),
            ts,
            name: event_name.to_string(),
            cpu,
            itid: target_utid,
        });
        self.tables.push_instant(InstantRow {
            ts,
            name: event_name.to_string(),
            ref_id: target_utid,
            wakeup_from,
            ref_type: Some("itid".to_string()),
            value: Some(0.0),
        });
    }

    fn on_sched_wakeup(&mut self, ts: i64, tid: u32, name: Option<&str>) -> Option<u32> {
        if tid == 0 {
            return None;
        }
        let utid = self.get_or_create_thread(ts, tid, name, None);
        self.pending_wakeup_by_tid.entry(tid).or_insert(ts);
        Some(utid)
    }

    fn check_wakeup_event(&mut self, tid: u32, utid: u32) {
        let Some(wakeup_ts) = self.pending_wakeup_by_tid.remove(&tid) else {
            return;
        };

        if let Some(row_id) = self.thread_state_open.get(&utid).copied() {
            let Some(row) = self.tables.thread_state_mut(row_id) else {
                return;
            };
            if row.state == "running" {
                return;
            }
            row.dur = Some(wakeup_ts.saturating_sub(row.ts));
        }

        let row_id = self.tables.push_thread_state(ThreadStateRow {
            utid,
            ts: wakeup_ts,
            dur: None,
            state: "runnable".to_string(),
            io_wait: None,
            blocked_function: None,
            waker_utid: None,
        });
        self.thread_state_open.insert(utid, row_id);
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

    fn measure_filter(&mut self, name: &str, filter_type: &str, cpu: Option<u32>) -> u64 {
        let key = (name.to_string(), filter_type.to_string(), cpu);
        if let Some(id) = self.measure_filters.get(&key) {
            return *id;
        }
        let id = self.next_measure_filter_id;
        self.next_measure_filter_id += 1;
        self.tables.push_measure_filter(MeasureFilterRow {
            id,
            name: name.to_string(),
            source_arg_set_id: None,
            filter_type: filter_type.to_string(),
        });
        if let Some(cpu) = cpu {
            self.tables.push_cpu_measure_filter(CpuMeasureFilterRow {
                id,
                name: name.to_string(),
                cpu,
            });
        }
        self.measure_filters.insert(key, id);
        id
    }

    fn push_measure(&mut self, ts: i64, filter_id: u64, value: i64) {
        if let Some(open_row) = self.open_measures.insert(filter_id, usize::MAX) {
            if open_row != usize::MAX {
                if let Some(row) = self.tables.measure_mut(open_row) {
                    row.dur = Some(ts.saturating_sub(row.ts));
                }
            }
        }
        let row_id = self.tables.push_measure(MeasureRow {
            measure_type: "measure".to_string(),
            ts,
            dur: None,
            value,
            filter_id,
        });
        self.open_measures.insert(filter_id, row_id);
    }

    fn get_or_create_thread(
        &mut self,
        ts: i64,
        tid: u32,
        name: Option<&str>,
        process_pid: Option<u32>,
    ) -> u32 {
        if let Some(info) = self.threads_by_tid.get_mut(&tid) {
            if let Some(name) = name.filter(|s| !s.is_empty()) {
                info.name = Some(name.to_string());
            }
            info.end_ts = Some(ts);
            return info.utid;
        }

        let upid = self.get_or_create_process(ts, process_pid.unwrap_or(tid), name);
        let id = self.next_id;
        self.next_id += 1;
        let name = name
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| (tid == 0).then(|| "idle".to_string()));

        self.threads_by_tid.insert(
            tid,
            ThreadInfo {
                utid: id,
                tid,
                upid,
                name,
                end_ts: Some(ts),
            },
        );
        id
    }

    fn get_or_create_process(&mut self, ts: i64, pid: u32, name: Option<&str>) -> u32 {
        if let Some(info) = self.processes_by_pid.get_mut(&pid) {
            if let Some(name) = name.filter(|s| !s.is_empty()) {
                info.name = Some(name.to_string());
            }
            info.end_ts = Some(ts);
            return info.upid;
        }

        let upid = self.next_id;
        self.next_id += 1;
        let name = name
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| (pid == 0).then(|| "idle".to_string()));
        self.processes_by_pid.insert(
            pid,
            ProcessInfoState {
                upid,
                pid,
                name,
                start_ts: Some(ts),
                end_ts: Some(ts),
            },
        );
        upid
    }

    fn observe_ts(&mut self, ts: i64) {
        self.start_ts = Some(self.start_ts.map_or(ts, |current| current.min(ts)));
        self.end_ts = Some(self.end_ts.map_or(ts, |current| current.max(ts)));
    }

    fn finish(self) -> ParseResult<ParsedTrace> {
        let mut parser = self;
        for info in parser.processes_by_pid.values() {
            parser.tables.push_process(ProcessRow {
                upid: info.upid,
                pid: info.pid,
                name: info.name.clone(),
                start_ts: info.start_ts,
                end_ts: info.end_ts,
            });
        }
        for info in parser.threads_by_tid.values() {
            parser.tables.push_thread(ThreadRow {
                utid: info.utid,
                tid: info.tid,
                upid: info.upid,
                name: info.name.clone(),
                is_main: true,
            });
        }

        let trace_id = format!("bytrace:{:016x}", parser.input_hash);
        let tables = parser
            .tables
            .finish(
                trace_id.clone(),
                parser.start_ts,
                parser.end_ts,
                "boottime".to_string(),
            )
            .map_err(|err| {
                TraceEngineError::Engine(format!("failed to build Arrow tables: {err}"))
            })?;
        log::debug!(
            target: "trace_parser::bytrace",
            "parsed bytrace trace_id={} start_ts={:?} end_ts={:?} events={} skipped_lines={}",
            trace_id,
            parser.start_ts,
            parser.end_ts,
            parser.parsed_events,
            parser.skipped_lines
        );

        Ok(ParsedTrace {
            trace_id,
            start_ts: parser.start_ts,
            end_ts: parser.end_ts,
            clock_domain: "boottime".to_string(),
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
        self.parse_text(bytes)?;
        let parser = std::mem::take(self);
        parser.finish()
    }
}

pub fn looks_like_bytrace_text(bytes: &[u8]) -> bool {
    let sample_len = bytes.len().min(4096);
    let sample = &bytes[..sample_len];
    sample.starts_with(b"# TRACE:")
        || sample.starts_with(b"# tracer:")
        || sample
            .windows(b"# tracer:".len())
            .any(|window| window == b"# tracer:")
        || sample
            .windows(b"TIMESTAMP  FUNCTION".len())
            .any(|window| window == b"TIMESTAMP  FUNCTION")
}

fn parse_event_line(line: &str) -> Option<TextTraceEvent<'_>> {
    let line = line.trim_end();
    if line.trim().is_empty() || line.trim_start().starts_with('#') {
        return None;
    }

    let (prefix, rest) = line.split_once(": ")?;
    let (name, payload) = rest.split_once(": ").unwrap_or((rest, ""));
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    let cpu_start = prefix.rfind('[')?;
    let cpu_end = prefix[cpu_start..].find(']')? + cpu_start;
    let cpu = prefix[cpu_start + 1..cpu_end].trim().parse::<u32>().ok()?;
    let ts = prefix[cpu_end + 1..]
        .split_whitespace()
        .last()
        .and_then(timestamp_to_ns)?;

    let before_cpu = &prefix[..cpu_start];
    let tgid_start = before_cpu.rfind('(')?;
    let tgid_end = before_cpu[tgid_start..].find(')')? + tgid_start;
    let tgid = parse_u32(before_cpu[tgid_start + 1..tgid_end].trim());
    let task = before_cpu[..tgid_start].trim_end();
    let (comm, tid) = parse_task_and_tid(task)?;

    Some(TextTraceEvent {
        comm,
        tid,
        tgid,
        cpu,
        ts,
        name,
        payload: payload.trim(),
    })
}

fn parse_task_and_tid(task: &str) -> Option<(&str, u32)> {
    let (comm, tid) = task.rsplit_once('-')?;
    let tid = parse_u32(tid.trim())?;
    Some((comm.trim(), tid))
}

fn field_value<'a>(payload: &'a str, key: &str) -> Option<&'a str> {
    payload.split_whitespace().find_map(|part| {
        let (field, value) = part.split_once('=')?;
        (field == key).then(|| clean_value(value))
    })
}

fn clean_value(value: &str) -> &str {
    value.trim_matches(|ch: char| ch == ',' || ch == ')' || ch == '(')
}

fn parse_u32(value: &str) -> Option<u32> {
    clean_value(value).parse::<u32>().ok()
}

fn parse_u64(value: &str) -> Option<u64> {
    clean_value(value).parse::<u64>().ok()
}

fn parse_i32(value: &str) -> Option<i32> {
    clean_value(value).parse::<i32>().ok()
}

fn timestamp_to_ns(value: &str) -> Option<i64> {
    let (seconds, fraction) = value.split_once('.').unwrap_or((value, ""));
    let seconds = seconds.parse::<i64>().ok()?;
    let mut nanos = 0_i64;
    let mut scale = 100_000_000_i64;
    for ch in fraction.chars().take(9) {
        let digit = ch.to_digit(10)? as i64;
        nanos += digit * scale;
        scale /= 10;
    }
    Some(seconds.saturating_mul(1_000_000_000).saturating_add(nanos))
}

fn state_from_text(state: &str) -> String {
    match state.chars().next().unwrap_or('R') {
        'R' => "runnable".to_string(),
        'S' => "sleeping".to_string(),
        'D' => "uninterruptible".to_string(),
        'T' => "stopped".to_string(),
        't' => "traced".to_string(),
        'X' | 'Z' => "exit".to_string(),
        'P' => "parked".to_string(),
        'I' => "idle".to_string(),
        other => format!("state_{other}"),
    }
}

fn event_payload_json(payload: &str) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    for part in payload.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        object.insert(key.to_string(), json!(clean_value(value)));
    }
    if object.is_empty() {
        json!({ "payload": payload })
    } else {
        serde_json::Value::Object(object)
    }
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}
