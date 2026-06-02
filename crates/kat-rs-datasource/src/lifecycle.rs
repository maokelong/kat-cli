use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatasetSummary {
    pub dataset_id: String,
    pub source_count: usize,
    pub source_ids: Vec<String>,
}
