use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use trace_model::TRACE_TABLE_NAMES;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasourceQueryRequest {
    pub sql: String,
    pub required_tables: Vec<String>,
}

impl DatasourceQueryRequest {
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            required_tables: Vec::new(),
        }
    }
}

pub fn infer_required_tables(sql: &str) -> Vec<String> {
    let tokens = sql
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();

    TRACE_TABLE_NAMES
        .iter()
        .filter(|table| tokens.contains(**table))
        .map(|table| (*table).to_string())
        .collect()
}
