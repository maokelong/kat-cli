pub mod error;
pub mod options;
pub mod parser;
pub mod parsers;
pub mod plugins;
pub mod registry;

pub use error::{ParseResult, TraceEngineError, TraceResult};
pub use options::{ParseOptions, ParseOutcome};
pub use parser::HarmonyTraceParser;
pub use parsers::bytrace::BytraceParser;
pub use parsers::htrace::HtraceParser;
pub use registry::{
    detect_trace_format, htrace_parser, parse_trace_bytes, parse_trace_file, TraceFormat,
};

pub fn parse_trace_file_with_options(
    path: &std::path::Path,
    options: &ParseOptions,
) -> ParseResult<ParseOutcome> {
    registry::parse_trace_file_with_options(path, options)
}

pub fn parse_trace_bytes_with_options(
    bytes: &[u8],
    options: &ParseOptions,
) -> ParseResult<ParseOutcome> {
    registry::parse_trace_bytes_with_options(bytes, options)
}
