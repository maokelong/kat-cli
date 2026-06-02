use crate::ParseResult;
use std::path::Path;
use trace_model::ParsedTrace;

pub trait HarmonyTraceParser {
    fn parse_file(&mut self, path: &Path) -> ParseResult<ParsedTrace>;
    fn parse_bytes(&mut self, bytes: &[u8]) -> ParseResult<ParsedTrace>;
}
