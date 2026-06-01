use crate::{
    parser::{HarmonyTraceParser, ParseResult},
    parsers::{
        bytrace::looks_like_bytrace_text, bytrace::BytraceParser, hilog::looks_like_hilog_text,
        hilog::HilogParser, hisysevent::looks_like_hisysevent_text, hisysevent::HiSysEventParser,
        htrace::HtraceParser, perf::looks_like_perf, perf::PerfParser,
        rawtrace::looks_like_rawtrace, rawtrace::RawTraceParser,
    },
};
use htrace_model::ParsedTrace;
use std::{fs, path::Path};

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
    match detect_trace_format(bytes) {
        TraceFormat::Htrace => {
            let mut parser = HtraceParser::default();
            parser.parse_bytes(bytes)
        }
        TraceFormat::BytraceText => {
            let mut parser = BytraceParser::default();
            parser.parse_bytes(bytes)
        }
        TraceFormat::RawTrace => {
            let mut parser = RawTraceParser::default();
            parser.parse_bytes(bytes)
        }
        TraceFormat::HiSysEventText => {
            let mut parser = HiSysEventParser::default();
            parser.parse_bytes(bytes)
        }
        TraceFormat::Perf => {
            let mut parser = PerfParser::default();
            parser.parse_bytes(bytes)
        }
        TraceFormat::HilogText => {
            let mut parser = HilogParser::default();
            parser.parse_bytes(bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bytrace_text() {
        let text = b"ACCS0-2716  ( 2519) [000] d..5 168758.662877: sched_wakeup: comm=Binder:924_3 pid=1200 prio=120 target_cpu=001";
        assert_eq!(detect_trace_format(text), TraceFormat::BytraceText);
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
}
