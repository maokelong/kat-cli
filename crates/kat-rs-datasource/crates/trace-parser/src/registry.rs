use crate::{
    options::{
        ParseOptions, ParseOutcome, PARSE_PHASE_DETECT_FORMAT, PARSE_PHASE_DISPATCH,
        PARSE_PHASE_FILE_READ,
    },
    parser::HarmonyTraceParser,
    parsers::htrace::HtraceParser,
    ParseResult,
};
use std::collections::BTreeMap;
use std::{fs, path::Path, time::Instant};
use trace_model::ParsedTrace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceFormat {
    Htrace,
}

pub fn htrace_parser() -> HtraceParser {
    HtraceParser::default()
}

pub fn detect_trace_format(bytes: &[u8]) -> TraceFormat {
    let _ = bytes;
    TraceFormat::Htrace
}

pub fn parse_trace_file(path: &Path) -> ParseResult<ParsedTrace> {
    Ok(parse_trace_file_with_options(path, &ParseOptions::full())?.parsed)
}

pub fn parse_trace_bytes(bytes: &[u8]) -> ParseResult<ParsedTrace> {
    Ok(parse_trace_bytes_with_options(bytes, &ParseOptions::full())?.parsed)
}

pub fn parse_trace_file_with_options(
    path: &Path,
    options: &ParseOptions,
) -> ParseResult<ParseOutcome> {
    let read_started = Instant::now();
    let bytes = fs::read(path)?;
    let file_read_elapsed = read_started.elapsed();
    let mut outcome = parse_trace_bytes_with_options(&bytes, options)?;
    insert_phase(
        &mut outcome.phase_elapsed_ms,
        PARSE_PHASE_FILE_READ,
        file_read_elapsed,
    );
    Ok(outcome)
}

pub fn parse_trace_bytes_with_options(
    bytes: &[u8],
    _options: &ParseOptions,
) -> ParseResult<ParseOutcome> {
    let mut phase_elapsed_ms = BTreeMap::new();

    let detect_started = Instant::now();
    let format = detect_trace_format(bytes);
    insert_phase(
        &mut phase_elapsed_ms,
        PARSE_PHASE_DETECT_FORMAT,
        detect_started.elapsed(),
    );

    let dispatch_started = Instant::now();
    let parsed = match format {
        TraceFormat::Htrace => {
            let mut parser = HtraceParser::default();
            parser.parse_bytes(bytes)?
        }
    };
    insert_phase(
        &mut phase_elapsed_ms,
        PARSE_PHASE_DISPATCH,
        dispatch_started.elapsed(),
    );

    Ok(ParseOutcome {
        parsed,
        phase_elapsed_ms,
    })
}

fn insert_phase(
    phase_elapsed_ms: &mut BTreeMap<String, u64>,
    phase: &str,
    elapsed: std::time::Duration,
) {
    phase_elapsed_ms.insert(phase.to_string(), elapsed.as_millis() as u64);
}
