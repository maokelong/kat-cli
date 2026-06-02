use crate::DatasourceResult;
use serde::{Deserialize, Serialize};

const HTRACE_V1_SCHEMA_MANIFEST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/crates/trace-model/schema/trace.v1.json"
));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaManifest {
    pub schema_version: String,
    pub tables: Vec<TableSchema>,
}

impl SchemaManifest {
    pub fn table(&self, name: &str) -> Option<&TableSchema> {
        self.tables.iter().find(|table| table.name == name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnSchema>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnSchema {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    #[serde(default)]
    pub unit: Option<String>,
}

pub fn load_schema_manifest() -> DatasourceResult<SchemaManifest> {
    Ok(serde_json::from_str(HTRACE_V1_SCHEMA_MANIFEST)?)
}
