use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Profile {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub knowledge: Vec<String>,
    #[serde(default)]
    pub overview_atomics: Vec<String>,
    #[serde(default)]
    pub approved_strategies: Vec<String>,
    #[serde(default)]
    pub allowed_atomics: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RoleRouter {
    #[serde(default)]
    pub domains: Vec<RouteDomain>,
    pub default_domain: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RouteDomain {
    pub id: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Atomic {
    pub id: String,
    pub domain: String,
    pub engine: String,
    pub description: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, AtomicInput>,
    pub resources: AtomicResources,
    pub sql: String,
    pub outputs: AtomicOutputs,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AtomicInput {
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AtomicResources {
    pub timeout_ms: u64,
    pub max_rows: usize,
    pub max_result_bytes: usize,
    pub priority: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AtomicOutputs {
    #[serde(default)]
    pub columns: Vec<AtomicColumn>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AtomicColumn {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strategy {
    pub metadata: StrategyMetadata,
    pub body: String,
    pub path: std::path::PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct StrategyMetadata {
    pub id: String,
    pub domain: String,
    pub status: String,
    #[serde(default)]
    pub allowed_atomics: Vec<String>,
    #[serde(default)]
    pub review_required: bool,
}
