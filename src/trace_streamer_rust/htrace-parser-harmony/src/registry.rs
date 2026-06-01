use crate::{
    parser::{HarmonyTraceParser, ParseResult},
    parsers::{
        bytrace::looks_like_bytrace_text, bytrace::BytraceParser, hilog::looks_like_hilog_text,
        hilog::HilogParser, hisysevent::looks_like_hisysevent_text, hisysevent::HiSysEventParser,
        htrace::HtraceParser, perf::looks_like_perf, perf::PerfParser,
        rawtrace::looks_like_rawtrace, rawtrace::RawTraceParser,
    },
};
use flate2::read::ZlibDecoder;
use htrace_core::TraceEngineError;
use htrace_model::ParsedTrace;
use std::{
    borrow::Cow,
    fs,
    io::{Cursor, Read},
    path::Path,
};

const MAX_TRACE_UNWRAP_DEPTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceFormat {
    Htrace,
    BytraceText,
    RawTrace,
    HiSysEventText,
    Perf,
    HilogText,
}

pub fn htrace_parser() -> HtraceParser {
    HtraceParser::default()
}

pub fn detect_trace_format(bytes: &[u8]) -> TraceFormat {
    let bytes = unwrap_trace_bytes(bytes).unwrap_or(Cow::Borrowed(bytes));
    detect_unwrapped_trace_format(&bytes)
}

fn detect_unwrapped_trace_format(bytes: &[u8]) -> TraceFormat {
    if looks_like_bytrace_text(bytes) {
        TraceFormat::BytraceText
    } else if looks_like_rawtrace(bytes) {
        TraceFormat::RawTrace
    } else if looks_like_perf(bytes) {
        TraceFormat::Perf
    } else if looks_like_hisysevent_text(bytes) {
        TraceFormat::HiSysEventText
    } else if looks_like_hilog_text(bytes) {
        TraceFormat::HilogText
    } else {
        TraceFormat::Htrace
    }
}

pub fn parse_trace_file(path: &Path) -> ParseResult<ParsedTrace> {
    let bytes = fs::read(path)?;
    parse_trace_bytes(&bytes)
}

pub fn parse_trace_bytes(bytes: &[u8]) -> ParseResult<ParsedTrace> {
    let bytes = unwrap_trace_bytes(bytes)?;
    match detect_unwrapped_trace_format(&bytes) {
        TraceFormat::Htrace => {
            let mut parser = HtraceParser::default();
            parser.parse_bytes(&bytes)
        }
        TraceFormat::BytraceText => {
            let mut parser = BytraceParser::default();
            parser.parse_bytes(&bytes)
        }
        TraceFormat::RawTrace => {
            let mut parser = RawTraceParser::default();
            parser.parse_bytes(&bytes)
        }
        TraceFormat::HiSysEventText => {
            let mut parser = HiSysEventParser::default();
            parser.parse_bytes(&bytes)
        }
        TraceFormat::Perf => {
            let mut parser = PerfParser::default();
            parser.parse_bytes(&bytes)
        }
        TraceFormat::HilogText => {
            let mut parser = HilogParser::default();
            parser.parse_bytes(&bytes)
        }
    }
}

fn unwrap_trace_bytes(bytes: &[u8]) -> ParseResult<Cow<'_, [u8]>> {
    let mut current = Cow::Borrowed(bytes);

    for _ in 0..MAX_TRACE_UNWRAP_DEPTH {
        if looks_like_zip(&current) {
            current = Cow::Owned(read_zip_trace_payload(&current)?);
            continue;
        }

        if looks_like_zlib(&current) {
            current = Cow::Owned(inflate_zlib_trace_payload(&current)?);
            continue;
        }

        return Ok(current);
    }

    Err(TraceEngineError::Parse(format!(
        "trace wrapper nesting exceeds limit {MAX_TRACE_UNWRAP_DEPTH}"
    )))
}

fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

fn looks_like_zlib(bytes: &[u8]) -> bool {
    if bytes.len() < 2 {
        return false;
    }

    let cmf = bytes[0];
    let flg = bytes[1];
    let compression_method = cmf & 0x0f;
    let compression_info = cmf >> 4;
    let header = u16::from(cmf) << 8 | u16::from(flg);

    compression_method == 8 && compression_info <= 7 && header % 31 == 0
}

fn inflate_zlib_trace_payload(bytes: &[u8]) -> ParseResult<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(bytes);
    let mut decoded = Vec::new();
    decoder.read_to_end(&mut decoded).map_err(|err| {
        TraceEngineError::Parse(format!("failed to decode zlib trace wrapper: {err}"))
    })?;
    if decoded.is_empty() {
        return Err(TraceEngineError::Parse(
            "zlib trace wrapper decoded to empty payload".to_string(),
        ));
    }
    Ok(decoded)
}

fn read_zip_trace_payload(bytes: &[u8]) -> ParseResult<Vec<u8>> {
    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|err| {
        TraceEngineError::Parse(format!("failed to open zip trace wrapper: {err}"))
    })?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|err| {
            TraceEngineError::Parse(format!("failed to read zip trace entry {index}: {err}"))
        })?;
        if entry.is_dir() || entry.name().starts_with("__MACOSX/") {
            continue;
        }

        let mut decoded = Vec::with_capacity(entry.size().min(usize::MAX as u64) as usize);
        entry.read_to_end(&mut decoded).map_err(|err| {
            TraceEngineError::Parse(format!(
                "failed to decode zip trace entry {}: {err}",
                entry.name()
            ))
        })?;
        if !decoded.is_empty() {
            return Ok(decoded);
        }
    }

    Err(TraceEngineError::Parse(
        "zip trace wrapper contains no non-empty trace entry".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    const BYTRACE_SAMPLE: &[u8] = b"ACCS0-2716  ( 2519) [000] d..5 168758.662877: sched_wakeup: comm=Binder:924_3 pid=1200 prio=120 target_cpu=001\n";

    #[test]
    fn detects_bytrace_text() {
        assert_eq!(detect_trace_format(BYTRACE_SAMPLE), TraceFormat::BytraceText);
    }

    #[test]
    fn defaults_unknown_bytes_to_htrace() {
        assert_eq!(
            detect_trace_format(b"\x00\x01\x02\x03"),
            TraceFormat::Htrace
        );
    }

    #[test]
    fn detects_remaining_top_level_formats() {
        assert_eq!(detect_trace_format(b"PERFILE2...."), TraceFormat::Perf);
        assert_eq!(detect_trace_format(&[0x49, 0xdf]), TraceFormat::RawTrace);
        assert_eq!(
            detect_trace_format(br#"{"domain_":"POWER","name_":"POWER_IDE_CPU","time_":1}"#),
            TraceFormat::HiSysEventText
        );
        assert_eq!(
            detect_trace_format(
                b"08-07 11:04:45.947   523   640 E C04200/Root: <205>cannot find windowNode\n"
            ),
            TraceFormat::HilogText
        );
    }

    #[test]
    fn unwraps_zlib_trace_before_detection_and_parse() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(BYTRACE_SAMPLE).unwrap();
        let compressed = encoder.finish().unwrap();

        assert_eq!(detect_trace_format(&compressed), TraceFormat::BytraceText);

        let parsed = parse_trace_bytes(&compressed).unwrap();
        assert_eq!(parsed.tables.raw_event.num_rows(), 1);
    }

    #[test]
    fn unwraps_zip_trace_before_detection_and_parse() {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer.start_file("htrace.txt", options).unwrap();
        writer.write_all(BYTRACE_SAMPLE).unwrap();
        let compressed = writer.finish().unwrap().into_inner();

        assert_eq!(detect_trace_format(&compressed), TraceFormat::BytraceText);

        let parsed = parse_trace_bytes(&compressed).unwrap();
        assert_eq!(parsed.tables.raw_event.num_rows(), 1);
    }
}
