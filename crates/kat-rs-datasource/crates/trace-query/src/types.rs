use crate::TraceResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const SCHEMA_VERSION: &str = "htrace.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceInput {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenOptions {
    pub cache_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceHandle {
    pub trace_id: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub sql: String,
    pub max_inline_rows: usize,
}

impl QueryRequest {
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            max_inline_rows: 10_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryColumn {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryStats {
    pub rows_returned: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub status: String,
    pub schema_version: String,
    pub columns: Vec<QueryColumn>,
    pub rows: Vec<Value>,
    pub stats: QueryStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInspection {
    pub available: bool,
    pub row_count: usize,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceInspection {
    pub schema_version: String,
    pub trace_id: String,
    pub path: PathBuf,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub clock_domain: String,
    pub tables: BTreeMap<String, TableInspection>,
}

#[async_trait]
pub trait TraceQueryEngine: Send + Sync {
    async fn open(&self, input: TraceInput, options: OpenOptions) -> TraceResult<TraceHandle>;
    async fn inspect(&self, handle: &TraceHandle) -> TraceResult<TraceInspection>;
    async fn query(&self, handle: &TraceHandle, request: QueryRequest) -> TraceResult<QueryResult>;
    async fn close(&self, handle: TraceHandle) -> TraceResult<()>;
}
