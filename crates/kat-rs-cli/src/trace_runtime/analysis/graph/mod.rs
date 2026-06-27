pub mod binding;
pub mod predicate;
pub mod spec;

use serde_json::Value;

#[derive(Clone, Debug)]
pub struct GraphCandidate {
    pub provider_id: String,
    pub input_table: String,
    pub relation: String,
    pub source: Value,
    pub row: Value,
    pub node: Value,
    pub annotations: Value,
    pub evidence_tables: Vec<String>,
}
