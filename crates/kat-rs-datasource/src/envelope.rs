use crate::metrics::QueryMetrics;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueryStatus {
    Ok,
    EmptyResult,
    UnsupportedSchema,
    UnsupportedSql,
    InvalidParams,
    Timeout,
    ResultTooLarge,
    ParseError,
    EngineError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryColumn {
    pub name: String,
    pub data_type: String,
    pub unit: Option<String>,
    pub nullable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryStats {
    pub rows_returned: usize,
    pub bytes_inline: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryEnvelope {
    pub status: QueryStatus,
    pub schema_version: String,
    pub dataset_id: String,
    pub columns: Vec<QueryColumn>,
    pub rows: Vec<Value>,
    pub stats: QueryStats,
    pub metrics: QueryMetrics,
    pub diagnostics: Vec<String>,
}
