use htrace_model::ParsedTrace;
use std::path::Path;

pub type ParseResult<T> = Result<T, htrace_core::TraceEngineError>;

pub trait HarmonyTraceParser {
    fn parse_file(&mut self, path: &Path) -> ParseResult<ParsedTrace>;
    fn parse_bytes(&mut self, bytes: &[u8]) -> ParseResult<ParsedTrace>;
}
