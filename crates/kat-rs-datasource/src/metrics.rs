use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub const PHASE_OPEN_DATASET: &str = "open_dataset";
pub const PHASE_PARSE_SOURCE: &str = "parse_source";
pub const PHASE_SESSION_LOOKUP: &str = "session_lookup";
pub const PHASE_SESSION_BUILD: &str = "session_build";
pub const PHASE_QUERY_EXECUTE: &str = "query_execute";
pub const PHASE_RESULT_SERIALIZE: &str = "result_serialize";

#[derive(Debug, Default)]
pub struct PhaseMetrics {
    phase_elapsed_ms: BTreeMap<String, u64>,
}

impl PhaseMetrics {
    pub fn record(&mut self, name: &'static str, elapsed: Duration) {
        self.record_ms(name, elapsed.as_millis() as u64);
    }

    pub fn record_ms(&mut self, name: &'static str, elapsed_ms: u64) {
        self.phase_elapsed_ms.insert(name.to_string(), elapsed_ms);
    }

    pub fn extend(&mut self, phases: &BTreeMap<String, u64>) {
        self.phase_elapsed_ms.extend(
            phases
                .iter()
                .map(|(name, elapsed)| (name.clone(), *elapsed)),
        );
    }

    pub fn into_inner(self) -> BTreeMap<String, u64> {
        self.phase_elapsed_ms
    }
}

pub fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryMetrics {
    pub elapsed_ms: u64,
    pub phase_elapsed_ms: BTreeMap<String, u64>,
    pub rows_returned: usize,
    pub bytes_inline: usize,
}
