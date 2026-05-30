pub mod mock;
pub mod perfetto_shell;

use crate::config::models::AtomicResources;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryEnvelope {
    pub status: String,
    pub atomic_id: String,
    pub engine: EngineInfo,
    pub trace: TraceInfo,
    pub rows: Vec<BTreeMap<String, Value>>,
    pub artifacts: Vec<ArtifactInfo>,
    pub stats: QueryStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceInfo {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    pub path: String,
    pub format: String,
    pub row_count: usize,
    pub byte_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryStats {
    pub rows_returned: usize,
    pub truncated: bool,
}

pub trait TraceQueryEngine {
    fn query(
        &self,
        atomic_id: &str,
        trace_path: &Path,
        sql: &str,
        resources: &AtomicResources,
    ) -> Result<QueryEnvelope>;
}
