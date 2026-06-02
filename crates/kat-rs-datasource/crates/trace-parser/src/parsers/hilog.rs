use crate::TraceEngineError;
use crate::{HarmonyTraceParser, ParseResult};
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use trace_model::{LogRow, ParsedTrace, ProcessRow, RawEventRow, ThreadRow, TraceTableBuilder};

#[derive(Debug, Clone)]
struct ThreadInfo {
    utid: u32,
    tid: u32,
    upid: u32,
}

#[derive(Default)]
pub struct HilogParser {
    tables: TraceTableBuilder,
    processes_by_pid: BTreeMap<u32, u32>,
    threads_by_tid: BTreeMap<u32, ThreadInfo>,
    next_process_id: u32,
    next_thread_id: u32,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    input_hash: u64,
}

impl HilogParser {
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
            .push_metadata("source_format", Some("hilog_text"));
    }

    fn parse_text(&mut self, text: &str) {
        for (line_index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            match parse_hilog_line(trimmed, line_index as u64) {
                Some(row) => {
                    self.observe_ts(row.ts);
                    self.get_or_create_thread(row.ts, row.pid, row.tid);
                    self.tables.push_log(row);
                }
                None => self.tables.push_raw_event(RawEventRow {
                    ts: None,
                    cpu: None,
                    tid: None,
                    event_name: "malformed_hilog_line".to_string(),
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
    }

    fn get_or_create_thread(&mut self, ts: i64, pid: u32, tid: u32) -> u32 {
        let upid = if let Some(upid) = self.processes_by_pid.get(&pid) {
            *upid
        } else {
            let upid = self.next_process_id;
            self.next_process_id += 1;
            self.processes_by_pid.insert(pid, upid);
            upid
        };

        if let Some(info) = self.threads_by_tid.get_mut(&tid) {
            let _ = ts;
            return info.utid;
        }

        let utid = self.next_thread_id;
        self.next_thread_id += 1;
        self.threads_by_tid
            .insert(tid, ThreadInfo { utid, tid, upid });
        utid
    }

    fn observe_ts(&mut self, ts: i64) {
        self.start_ts = Some(self.start_ts.map_or(ts, |current| current.min(ts)));
        self.end_ts = Some(self.end_ts.map_or(ts, |current| current.max(ts)));
    }

    fn finish(mut self) -> ParseResult<ParsedTrace> {
        for (pid, upid) in self.processes_by_pid {
            self.tables.push_process(ProcessRow {
                upid,
                pid,
                name: None,
                start_ts: self.start_ts,
                end_ts: self.end_ts,
            });
        }
        for info in self.threads_by_tid.values() {
            self.tables.push_thread(ThreadRow {
                utid: info.utid,
                tid: info.tid,
                upid: info.upid,
                name: None,
                is_main: info.tid == info.upid,
            });
        }

        let trace_id = format!("hilog:{:016x}", self.input_hash);
        let tables = self
            .tables
            .finish(
                trace_id.clone(),
                self.start_ts,
                self.end_ts,
                "realtime".to_string(),
            )
            .map_err(|err| {
                TraceEngineError::Engine(format!("failed to build Arrow tables: {err}"))
            })?;

        Ok(ParsedTrace {
            trace_id,
            start_ts: self.start_ts,
            end_ts: self.end_ts,
            clock_domain: "realtime".to_string(),
            tables,
        })
    }
}

impl HarmonyTraceParser for HilogParser {
    fn parse_file(&mut self, path: &Path) -> ParseResult<ParsedTrace> {
        let bytes = fs::read(path)?;
        self.parse_bytes(&bytes)
    }

    fn parse_bytes(&mut self, bytes: &[u8]) -> ParseResult<ParsedTrace> {
        self.reset_for_input(bytes);
        let text = String::from_utf8_lossy(bytes);
        self.parse_text(&text);
        let parser = std::mem::take(self);
        parser.finish()
    }
}

pub(crate) fn looks_like_hilog_text(bytes: &[u8]) -> bool {
    let sample_len = bytes.len().min(64 * 1024);
    if sample_len == 0 {
        return false;
    }

    let text = String::from_utf8_lossy(&bytes[..sample_len]);
    text.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty() && parse_hilog_line(trimmed, 0).is_some()
    })
}

fn parse_hilog_line(line: &str, seq: u64) -> Option<LogRow> {
    let (head, context) = line.split_once(": ").or_else(|| line.split_once(':'))?;
    let tokens = head.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 5 {
        return None;
    }

    let tag = tokens.last()?.to_string();
    let level = tokens.get(tokens.len() - 2)?.to_string();
    if !matches!(level.as_str(), "F" | "E" | "W" | "I" | "D") {
        return None;
    }

    let tid = tokens.get(tokens.len() - 3)?.parse::<u32>().ok()?;
    let pid = tokens.get(tokens.len() - 4)?.parse::<u32>().ok()?;
    let time_tokens = &tokens[..tokens.len() - 4];
    let ts = parse_hilog_time(time_tokens)?;

    Some(LogRow {
        seq,
        ts,
        pid,
        tid,
        level,
        tag,
        context: context.to_string(),
        origints: ts,
    })
}

fn parse_hilog_time(tokens: &[&str]) -> Option<i64> {
    match tokens {
        [seconds] => parse_seconds_to_ns(seconds),
        [date, time] => parse_local_datetime_to_ns(None, date, time),
        [_zone, date, time] => parse_local_datetime_to_ns(None, date, time),
        _ => None,
    }
}

fn parse_seconds_to_ns(value: &str) -> Option<i64> {
    let (sec, frac) = value.split_once('.')?;
    if !sec.chars().all(|ch| ch.is_ascii_digit()) || !frac.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    let sec = sec.parse::<i64>().ok()?;
    let mut nsec = frac.parse::<i64>().ok()?;
    match frac.len() {
        3 => nsec *= 1_000_000,
        6 => nsec *= 1_000,
        len if len < 9 => nsec *= 10_i64.pow((9 - len) as u32),
        len if len > 9 => nsec /= 10_i64.pow((len - 9) as u32),
        _ => {}
    }
    Some(sec.saturating_mul(1_000_000_000).saturating_add(nsec))
}

fn parse_local_datetime_to_ns(year: Option<i32>, date: &str, time: &str) -> Option<i64> {
    let date_parts = date.split('-').collect::<Vec<_>>();
    let (year, month, day) = match date_parts.as_slice() {
        [month, day] => (
            year.unwrap_or_else(|| Local::now().year()),
            month.parse::<u32>().ok()?,
            day.parse::<u32>().ok()?,
        ),
        [year, month, day] => (
            year.parse::<i32>().ok()?,
            month.parse::<u32>().ok()?,
            day.parse::<u32>().ok()?,
        ),
        _ => return None,
    };

    let (hms, frac) = time.split_once('.')?;
    let hms = hms.split(':').collect::<Vec<_>>();
    let [hour, minute, second] = hms.as_slice() else {
        return None;
    };
    let mut nsec = frac.parse::<u32>().ok()?;
    match frac.len() {
        3 => nsec *= 1_000_000,
        6 => nsec *= 1_000,
        len if len < 9 => nsec *= 10_u32.pow((9 - len) as u32),
        len if len > 9 => nsec /= 10_u32.pow((len - 9) as u32),
        _ => {}
    }

    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let time = NaiveTime::from_hms_nano_opt(
        hour.parse::<u32>().ok()?,
        minute.parse::<u32>().ok()?,
        second.parse::<u32>().ok()?,
        nsec,
    )?;
    let naive = NaiveDateTime::new(date, time);
    let local = Local
        .from_local_datetime(&naive)
        .single()
        .or_else(|| Local.from_local_datetime(&naive).earliest())?;
    Some(
        local
            .timestamp()
            .saturating_mul(1_000_000_000)
            .saturating_add(local.timestamp_subsec_nanos() as i64),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hilog_text_variants() {
        let text = "08-07 11:04:45.947   523   640 E C04200/Root: <205>cannot find windowNode\n\
            CST 08-05 17:41:00.039   955   955 I C03900/Ace: child size is empty\n\
            CST 2017-08-05 17:41:19.409   840   926 I C01560/Wifi: thread work normally\n\
            1501926013.969  1585  1585 I C02d10/HiView-DOCDB: close ejdb success\n\
            2337.006   601   894 E C01200/Ces: permission denied\n";

        let parsed = HilogParser::default()
            .parse_bytes(text.as_bytes())
            .expect("parse hilog");
        assert_eq!(parsed.tables.log.num_rows(), 5);
        assert_eq!(parsed.tables.thread.num_rows(), 5);
    }

    #[test]
    fn detects_hilog_text() {
        assert!(looks_like_hilog_text(
            b"08-07 11:04:45.947   523   640 E C04200/Root: <205>cannot find windowNode\n"
        ));
    }
}
