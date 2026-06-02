use crate::inspection::ColumnInspection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TableAvailability {
    Available,
    Empty,
    PluginAbsent,
    ParserNotImplemented,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableCapability {
    pub available: bool,
    pub availability: TableAvailability,
    pub row_count: usize,
    pub reason: Option<String>,
    pub columns: Vec<ColumnInspection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetInspection {
    pub schema_version: String,
    pub dataset_id: String,
    pub source_count: usize,
    pub tables: BTreeMap<String, TableCapability>,
}
