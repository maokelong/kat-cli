use crate::TraceEngineError;
use crate::{HarmonyTraceParser, ParseResult};
use serde_json::{json, Map, Value};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use trace_model::{
    HiSysEventAllRow, HiSysEventMeasureRow, ParsedTrace, RawEventRow, TraceTableBuilder,
};

#[derive(Default)]
pub struct HiSysEventParser {
    tables: TraceTableBuilder,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    input_hash: u64,
}

impl HiSysEventParser {
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
            .push_metadata("source_format", Some("hisysevent_text"));
    }

    fn parse_text(&mut self, text: &str) {
        for (line_index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            match serde_json::from_str::<Value>(trimmed) {
                Ok(Value::Object(object)) => self.push_event(line_index as u64, object),
                Ok(value) => self.push_malformed(line_index + 1, trimmed, Some(value)),
                Err(_) => self.push_malformed(line_index + 1, trimmed, None),
            }
        }
    }

    fn push_event(&mut self, id: u64, object: Map<String, Value>) {
        let ts = object
            .get("time_")
            .and_then(value_to_i64)
            .map(|ms| ms.saturating_mul(1_000_000));
        if let Some(ts) = ts {
            self.observe_ts(ts);
        }

        let domain = object.get("domain_").and_then(value_to_string);
        let event_name = object.get("name_").and_then(value_to_string);
        let contents = object
            .iter()
            .filter(|(key, _)| !is_hisysevent_reserved_key(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Map<_, _>>();

        self.tables.push_hisysevent_all_event(HiSysEventAllRow {
            id,
            domain,
            event_name: event_name.clone(),
            ts,
            event_type: object.get("type_").and_then(value_to_i64),
            time_zone: object.get("tz_").and_then(value_to_string),
            pid: object.get("pid_").and_then(value_to_i64),
            tid: object.get("tid_").and_then(value_to_i64),
            uid: object.get("uid_").and_then(value_to_i64),
            level: object.get("level_").and_then(value_to_string),
            tag: object.get("tag_").and_then(value_to_string),
            event_id: object.get("id_").and_then(value_to_string),
            seq: object.get("seq_").and_then(value_to_i64),
            info: object.get("info_").and_then(value_to_string),
            contents: Some(Value::Object(contents.clone()).to_string()),
        });

        for (key, value) in object
            .iter()
            .filter(|(key, _)| is_hisysevent_measure_key(key))
        {
            self.push_measure_values(id, ts, event_name.as_deref(), key, value);
        }
    }

    fn push_measure_values(
        &mut self,
        serial: u64,
        ts: Option<i64>,
        name: Option<&str>,
        key: &str,
        value: &Value,
    ) {
        if let Value::Array(values) = value {
            for item in values {
                self.push_measure_value(serial, ts, name, key, item);
            }
        } else {
            self.push_measure_value(serial, ts, name, key, value);
        }
    }

    fn push_measure_value(
        &mut self,
        serial: u64,
        ts: Option<i64>,
        name: Option<&str>,
        key: &str,
        value: &Value,
    ) {
        let id = self.tables.next_hisysevent_measure_id();
        if let Some(number) = value.as_f64() {
            self.tables.push_hisysevent_measure(HiSysEventMeasureRow {
                id,
                serial,
                ts,
                name: name.map(ToOwned::to_owned),
                key: key.to_string(),
                value_type: 0,
                int_value: Some(number),
                string_value: None,
            });
        } else {
            self.tables.push_hisysevent_measure(HiSysEventMeasureRow {
                id,
                serial,
                ts,
                name: name.map(ToOwned::to_owned),
                key: key.to_string(),
                value_type: 1,
                int_value: None,
                string_value: value_to_string(value),
            });
        }
    }

    fn push_malformed(&mut self, line: usize, text: &str, value: Option<Value>) {
        self.tables.push_raw_event(RawEventRow {
            ts: None,
            cpu: None,
            tid: None,
            event_name: "malformed_hisysevent_line".to_string(),
            payload_json: Some(
                json!({
                    "line": line,
                    "text": text,
                    "json": value
                })
                .to_string(),
            ),
        });
    }

    fn observe_ts(&mut self, ts: i64) {
        self.start_ts = Some(self.start_ts.map_or(ts, |current| current.min(ts)));
        self.end_ts = Some(self.end_ts.map_or(ts, |current| current.max(ts)));
    }

    fn finish(self) -> ParseResult<ParsedTrace> {
        let trace_id = format!("hisysevent:{:016x}", self.input_hash);
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

impl HarmonyTraceParser for HiSysEventParser {
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

pub(crate) fn looks_like_hisysevent_text(bytes: &[u8]) -> bool {
    let sample_len = bytes.len().min(64 * 1024);
    if sample_len == 0 {
        return false;
    }

    let text = String::from_utf8_lossy(&bytes[..sample_len]);
    text.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with('{')
            && serde_json::from_str::<Value>(trimmed)
                .ok()
                .and_then(|value| value.get("domain_").cloned())
                .is_some()
    })
}

fn is_hisysevent_reserved_key(key: &str) -> bool {
    matches!(
        key,
        "domain_"
            | "name_"
            | "type_"
            | "time_"
            | "tz_"
            | "pid_"
            | "tid_"
            | "uid_"
            | "id_"
            | "info_"
            | "tag_"
            | "level_"
            | "seq_"
    )
}

fn is_hisysevent_measure_key(key: &str) -> bool {
    !matches!(key, "name_" | "time_")
}

fn value_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_f64().map(|value| value as i64))
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hisysevent_json_lines() {
        let text = r#"{"domain_":"POWER","name_":"POWER_IDE_CPU","type_":1,"time_":1700000000000,"tz_":"+0800","pid_":1,"tid_":2,"uid_":3,"level_":"MINOR","tag_":"PowerStats","id_":"abc","seq_":7,"info_":"ok","APPNAME":"demo","VALUE":[1,2]}"#;

        let parsed = HiSysEventParser::default()
            .parse_bytes(text.as_bytes())
            .expect("parse hisysevent");
        assert_eq!(parsed.tables.hisysevent_all_event.num_rows(), 1);
        assert_eq!(parsed.tables.hisysevent_measure.num_rows(), 14);
    }

    #[test]
    fn detects_hisysevent_text() {
        assert!(looks_like_hisysevent_text(
            br#"{"domain_":"POWER","name_":"POWER_IDE_CPU","time_":1}"#
        ));
    }
}
