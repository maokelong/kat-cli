use serde::Deserialize;
use std::sync::OnceLock;

const TRACE_SCHEMA_MANIFEST_JSON: &str = include_str!("../schema/trace.v1.json");

static TRACE_SCHEMA_MANIFEST: OnceLock<TraceSchemaManifest> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
pub struct TraceSchemaManifest {
    pub schema_version: String,
    pub tables: Vec<TraceTableSchema>,
}

impl TraceSchemaManifest {
    pub fn table(&self, table_name: &str) -> Option<&TraceTableSchema> {
        self.tables.iter().find(|table| table.name == table_name)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TraceTableSchema {
    pub name: String,
    pub columns: Vec<TraceColumnSchema>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TraceColumnSchema {
    pub name: String,
    pub data_type: TraceDataType,
    pub nullable: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub enum TraceDataType {
    #[serde(rename = "Boolean")]
    Boolean,
    #[serde(rename = "Float64")]
    Float64,
    #[serde(rename = "Int32")]
    Int32,
    #[serde(rename = "Int64")]
    Int64,
    #[serde(rename = "UInt32")]
    UInt32,
    #[serde(rename = "UInt64")]
    UInt64,
    #[serde(rename = "Utf8")]
    Utf8,
}

impl TraceDataType {
    pub fn as_manifest_str(self) -> &'static str {
        match self {
            Self::Boolean => "Boolean",
            Self::Float64 => "Float64",
            Self::Int32 => "Int32",
            Self::Int64 => "Int64",
            Self::UInt32 => "UInt32",
            Self::UInt64 => "UInt64",
            Self::Utf8 => "Utf8",
        }
    }
}

pub fn schema_manifest() -> &'static TraceSchemaManifest {
    TRACE_SCHEMA_MANIFEST.get_or_init(|| {
        serde_json::from_str(TRACE_SCHEMA_MANIFEST_JSON)
            .expect("embedded trace schema manifest must be valid JSON")
    })
}
