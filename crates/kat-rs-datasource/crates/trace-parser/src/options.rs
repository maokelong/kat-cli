use std::collections::{BTreeMap, BTreeSet};
use trace_model::ParsedTrace;

pub const PARSE_PHASE_FILE_READ: &str = "parser.file_read";
pub const PARSE_PHASE_UNWRAP: &str = "parser.unwrap";
pub const PARSE_PHASE_DETECT_FORMAT: &str = "parser.detect_format";
pub const PARSE_PHASE_DISPATCH: &str = "parser.dispatch";
pub const PARSE_PHASE_BYTRACE_PARSE_LINES: &str = "parser.bytrace.parse_lines";
pub const PARSE_PHASE_BYTRACE_FINISH_INTERVALS: &str = "parser.bytrace.finish_intervals";
pub const PARSE_PHASE_BUILD_RECORD_BATCHES: &str = "parser.build_record_batches";

const BYTRACE_SCHEDULER_TABLES: &[&str] = &[
    "trace_metadata",
    "trace_bounds",
    "sched_slice",
    "thread_state",
    "thread",
    "process",
    "data_dict",
];

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

    pub fn wants_raw_events(&self) -> bool {
        self.wants_table("raw_event")
    }

    pub fn is_bytrace_scheduler_only(&self) -> bool {
        !self.required_tables.is_empty()
            && self.required_tables.iter().all(|table| {
                BYTRACE_SCHEDULER_TABLES
                    .iter()
                    .any(|allowed| table == allowed)
            })
    }
}

#[derive(Debug)]
pub struct ParseOutcome {
    pub parsed: ParsedTrace,
    pub phase_elapsed_ms: BTreeMap<String, u64>,
}
