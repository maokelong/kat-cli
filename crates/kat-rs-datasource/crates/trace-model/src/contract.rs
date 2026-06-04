use arrow_schema::{DataType, Field, Schema, SchemaRef};
use include_dir::{include_dir, Dir, DirEntry};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

static TABLE_CONTRACT_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/contracts/tables");
static TABLE_CONTRACTS: OnceLock<Vec<TraceTableContract>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
pub struct TraceTableContract {
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub columns: Vec<TraceColumnContract>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TraceColumnContract {
    pub name: String,
    pub data_type: TraceColumnType,
    pub nullable: bool,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub enum TraceColumnType {
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

impl TraceColumnType {
    pub fn as_contract_str(self) -> &'static str {
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

    pub fn arrow_data_type(self) -> DataType {
        match self {
            Self::Boolean => DataType::Boolean,
            Self::Float64 => DataType::Float64,
            Self::Int32 => DataType::Int32,
            Self::Int64 => DataType::Int64,
            Self::UInt32 => DataType::UInt32,
            Self::UInt64 => DataType::UInt64,
            Self::Utf8 => DataType::Utf8,
        }
    }
}

pub fn trace_table_contracts() -> &'static [TraceTableContract] {
    TABLE_CONTRACTS
        .get_or_init(|| {
            let mut contracts = Vec::new();
            collect_contracts(&TABLE_CONTRACT_DIR, &mut contracts);
            contracts.sort_by(|left, right| left.name.cmp(&right.name));
            validate_contract_names(&contracts);
            contracts
        })
        .as_slice()
}

pub fn trace_table_contract(table_name: &str) -> Option<&'static TraceTableContract> {
    trace_table_contracts()
        .iter()
        .find(|contract| contract.name == table_name)
}

pub fn trace_table_names() -> Vec<&'static str> {
    trace_table_contracts()
        .iter()
        .map(|contract| contract.name.as_str())
        .collect()
}

pub fn is_trace_table(table_name: &str) -> bool {
    trace_table_contract(table_name).is_some()
}

pub fn trace_table_schema(table_name: &str) -> Option<SchemaRef> {
    trace_table_contract(table_name).map(schema_from_contract)
}

fn collect_contracts(dir: &Dir<'_>, contracts: &mut Vec<TraceTableContract>) {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(child) => collect_contracts(child, contracts),
            DirEntry::File(file) => {
                if file.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
                    continue;
                }
                let content = file
                    .contents_utf8()
                    .expect("trace table contract must be UTF-8 JSON");
                let contract = serde_json::from_str::<TraceTableContract>(content)
                    .expect("trace table contract JSON must match TraceTableContract");
                contracts.push(contract);
            }
        }
    }
}

fn validate_contract_names(contracts: &[TraceTableContract]) {
    let mut names = BTreeSet::new();
    for contract in contracts {
        assert!(
            names.insert(contract.name.as_str()),
            "duplicate trace table contract: {}",
            contract.name
        );
    }
}

fn schema_from_contract(contract: &TraceTableContract) -> SchemaRef {
    let fields = contract
        .columns
        .iter()
        .map(|column| {
            Field::new(
                column.name.as_str(),
                column.data_type.arrow_data_type(),
                column.nullable,
            )
        })
        .collect::<Vec<_>>();

    Arc::new(Schema::new(fields))
}
