use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSource {
    pub path: PathBuf,
    pub format_hint: Option<String>,
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetInput {
    pub sources: Vec<TraceSource>,
    pub cache_dir: Option<PathBuf>,
    pub required_tables: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceHandle {
    pub source_id: String,
    pub trace_id: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetHandle {
    pub dataset_id: String,
    pub sources: Vec<SourceHandle>,
}
