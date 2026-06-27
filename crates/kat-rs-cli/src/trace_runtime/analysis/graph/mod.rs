pub mod binding;
pub mod evidence;
pub mod expand;
pub mod number;
pub mod predicate;
pub mod select;
pub mod spec;
pub mod walk;

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
