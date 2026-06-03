use std::collections::{BTreeMap, BTreeSet};
use trace_model::ParsedTrace;

pub const PARSE_PHASE_FILE_READ: &str = "parser.file_read";
pub const PARSE_PHASE_DETECT_FORMAT: &str = "parser.detect_format";
pub const PARSE_PHASE_DISPATCH: &str = "parser.dispatch";
pub const PARSE_PHASE_BUILD_RECORD_BATCHES: &str = "parser.build_record_batches";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParseOptions {
    required_tables: BTreeSet<String>,
}

impl ParseOptions {
    pub fn full() -> Self {
        Self::default()
    }

    pub fn for_required_tables<I, S>(tables: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            required_tables: tables
                .into_iter()
                .map(|table| table.as_ref().to_ascii_lowercase())
                .collect(),
        }
    }

    pub fn required_tables(&self) -> &BTreeSet<String> {
        &self.required_tables
    }

    pub fn wants_table(&self, table: &str) -> bool {
        self.required_tables.is_empty() || self.required_tables.contains(table)
    }
}

#[derive(Debug)]
pub struct ParseOutcome {
    pub parsed: ParsedTrace,
    pub phase_elapsed_ms: BTreeMap<String, u64>,
}
