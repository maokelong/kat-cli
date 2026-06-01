pub mod parser;
pub mod parsers;
pub mod plugins;
pub mod registry;

pub use parser::{HarmonyTraceParser, ParseResult};
pub use parsers::bytrace::BytraceParser;
pub use parsers::hilog::HilogParser;
pub use parsers::hisysevent::HiSysEventParser;
pub use parsers::htrace::HtraceParser;
pub use parsers::perf::PerfParser;
pub use parsers::rawtrace::RawTraceParser;
pub use registry::{
    detect_trace_format, htrace_parser, parse_trace_bytes, parse_trace_file, TraceFormat,
};
