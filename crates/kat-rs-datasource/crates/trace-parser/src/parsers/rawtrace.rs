use crate::TraceEngineError;
use crate::{HarmonyTraceParser, ParseResult};
use serde_json::json;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use trace_model::{ParsedTrace, RawEventRow, TraceTableBuilder};

const RAW_TRACE_MAGIC: u16 = 57161;
const RAW_TRACE_HEADER_SIZE: usize = 12;

#[derive(Default)]
pub struct RawTraceParser {
    tables: TraceTableBuilder,
    input_hash: u64,
}

impl RawTraceParser {
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
            .push_metadata("source_format", Some("raw_trace"));
    }

    fn parse_rawtrace(&mut self, bytes: &[u8]) -> ParseResult<()> {
        if bytes.starts_with(b"file_header:") {
            self.parse_rawtrace_text_dump(bytes);
            return Ok(());
        }

        if bytes.len() < RAW_TRACE_HEADER_SIZE || read_u16_le(bytes, 0)? != RAW_TRACE_MAGIC {
            return Err(TraceEngineError::Parse(
                "not a raw trace file: missing raw trace magic".to_string(),
            ));
        }

        let file_type = bytes.get(2).copied().unwrap_or_default();
        let version = read_u16_le(bytes, 4)?;
        let reserved = read_u32_le(bytes, 8)?;
        self.tables.push_raw_event(RawEventRow {
            ts: None,
            cpu: None,
            tid: None,
            event_name: "rawtrace_header".to_string(),
            payload_json: Some(
                json!({
                    "magic": RAW_TRACE_MAGIC,
                    "file_type": file_type,
                    "version": version,
                    "reserved": reserved,
                    "file_size": bytes.len()
                })
                .to_string(),
            ),
        });

        let mut offset = RAW_TRACE_HEADER_SIZE;
        let mut segment_id = 0u64;
        while offset + 8 <= bytes.len() {
            let content_type = read_u32_le(bytes, offset)?;
            let len = read_u32_le(bytes, offset + 4)? as usize;
            offset += 8;
            if offset + len > bytes.len() {
                self.tables.push_raw_event(RawEventRow {
                    ts: None,
                    cpu: None,
                    tid: None,
                    event_name: "truncated_rawtrace_segment".to_string(),
                    payload_json: Some(
                        json!({
                            "segment": segment_id,
                            "content_type": content_type,
                            "declared_len": len,
                            "remaining": bytes.len().saturating_sub(offset)
                        })
                        .to_string(),
                    ),
                });
                break;
            }

            let payload = &bytes[offset..offset + len];
            self.tables.push_raw_event(RawEventRow {
                ts: None,
                cpu: rawtrace_cpu(content_type),
                tid: None,
                event_name: rawtrace_content_name(content_type).to_string(),
                payload_json: Some(
                    json!({
                        "segment": segment_id,
                        "content_type": content_type,
                        "offset": offset - 8,
                        "payload_len": len,
                        "text_preview": text_preview(payload)
                    })
                    .to_string(),
                ),
            });

            offset += len;
            segment_id += 1;
        }

        if offset < bytes.len() {
            self.tables.push_raw_event(RawEventRow {
                ts: None,
                cpu: None,
                tid: None,
                event_name: "rawtrace_trailing_bytes".to_string(),
                payload_json: Some(
                    json!({ "offset": offset, "len": bytes.len() - offset }).to_string(),
                ),
            });
        }

        Ok(())
    }

    fn parse_rawtrace_text_dump(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let mut current_section = None::<String>;
        let mut line_count = 0usize;
        for line in text.lines() {
            if let Some(section) = line.strip_suffix(':') {
                if let Some(section) = current_section.take() {
                    self.push_text_section(&section, line_count);
                }
                current_section = Some(section.to_string());
                line_count = 0;
            } else {
                line_count += 1;
            }
        }
        if let Some(section) = current_section {
            self.push_text_section(&section, line_count);
        }
    }

    fn push_text_section(&mut self, section: &str, line_count: usize) {
        self.tables.push_raw_event(RawEventRow {
            ts: None,
            cpu: None,
            tid: None,
            event_name: "rawtrace_text_section".to_string(),
            payload_json: Some(
                json!({
                    "section": section,
                    "line_count": line_count
                })
                .to_string(),
            ),
        });
    }

    fn finish(self) -> ParseResult<ParsedTrace> {
        let trace_id = format!("rawtrace:{:016x}", self.input_hash);
        let tables = self
            .tables
            .finish(trace_id.clone(), None, None, "unknown".to_string())
            .map_err(|err| {
                TraceEngineError::Engine(format!("failed to build Arrow tables: {err}"))
            })?;

        Ok(ParsedTrace {
            trace_id,
            start_ts: None,
            end_ts: None,
            clock_domain: "unknown".to_string(),
            tables,
        })
    }
}

impl HarmonyTraceParser for RawTraceParser {
    fn parse_file(&mut self, path: &Path) -> ParseResult<ParsedTrace> {
        let bytes = fs::read(path)?;
        self.parse_bytes(&bytes)
    }

    fn parse_bytes(&mut self, bytes: &[u8]) -> ParseResult<ParsedTrace> {
        self.reset_for_input(bytes);
        self.parse_rawtrace(bytes)?;
        let parser = std::mem::take(self);
        parser.finish()
    }
}

pub(crate) fn looks_like_rawtrace(bytes: &[u8]) -> bool {
    bytes.starts_with(b"file_header:")
        || (bytes.len() >= 2
            && u16::from_le_bytes(bytes[0..2].try_into().expect("slice has length 2"))
                == RAW_TRACE_MAGIC)
}

fn rawtrace_content_name(content_type: u32) -> &'static str {
    match content_type {
        1 => "rawtrace_event_formats",
        2 => "rawtrace_cmdlines",
        3 => "rawtrace_tgids",
        4..=29 => "rawtrace_cpu_raw",
        30 => "rawtrace_header_page",
        31 => "rawtrace_printk_formats",
        32 => "rawtrace_kallsyms",
        _ => "rawtrace_unknown_segment",
    }
}

fn rawtrace_cpu(content_type: u32) -> Option<u32> {
    (4..30)
        .contains(&content_type)
        .then_some(content_type.saturating_sub(4))
}

fn text_preview(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let max_len = bytes.len().min(160);
    let text = String::from_utf8_lossy(&bytes[..max_len]);
    Some(text.replace('\0', "\\0"))
}

fn read_u16_le(bytes: &[u8], offset: usize) -> ParseResult<u16> {
    let end = offset + 2;
    let data = bytes
        .get(offset..end)
        .ok_or_else(|| TraceEngineError::Parse(format!("missing u16 at byte {offset}")))?;
    Ok(u16::from_le_bytes(
        data.try_into().expect("slice has length 2"),
    ))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> ParseResult<u32> {
    let end = offset + 4;
    let data = bytes
        .get(offset..end)
        .ok_or_else(|| TraceEngineError::Parse(format!("missing u32 at byte {offset}")))?;
    Ok(u32::from_le_bytes(
        data.try_into().expect("slice has length 4"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_rawtrace_segments() {
        let mut bytes = vec![0x49, 0xdf, 0, 0, 1, 0, 0, 0, 9, 0, 0, 0];
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(b"hello");

        let parsed = RawTraceParser::default()
            .parse_bytes(&bytes)
            .expect("parse rawtrace");
        assert_eq!(parsed.tables.raw_event.num_rows(), 2);
    }

    #[test]
    fn detects_rawtrace() {
        assert!(looks_like_rawtrace(&[0x49, 0xdf]));
        assert!(looks_like_rawtrace(b"file_header:\n"));
    }

    #[test]
    fn parses_repository_rawtrace_fixture() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../tests/fixtures/traces/rawtrace.bin");
        if !fixture.exists() {
            eprintln!("skip missing fixture {}", fixture.display());
            return;
        }

        let parsed = RawTraceParser::default()
            .parse_file(&fixture)
            .expect("parse rawtrace fixture");
        assert!(parsed.tables.raw_event.num_rows() > 0);
    }
}
