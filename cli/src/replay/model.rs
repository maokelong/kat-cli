use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReplayPlan {
    pub problem_signature: String,
    pub source_strategy: String,
    #[serde(default)]
    pub steps: Vec<ReplayStep>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReplayStep {
    pub atomic: String,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayRunSummary {
    pub problem_signature: String,
    pub source_strategy: String,
    pub step_count: usize,
    pub statuses: Vec<String>,
}
