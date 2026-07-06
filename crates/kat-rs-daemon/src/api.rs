use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct DataEnvelope<T> {
    pub data: T,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PaginatedEnvelope<T> {
    pub data: Vec<T>,
    pub pagination: Pagination,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Pagination {
    pub limit: usize,
    pub offset: usize,
    pub total_items: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: &'static str,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DatasourceSource {
    Hitrace,
    LangfuseLegacy,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, ToSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InputRole {
    File,
    Observations,
    Traces,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(tag = "source", deny_unknown_fields)]
pub enum CreateDatasourceRequest {
    #[serde(rename = "HITRACE")]
    Hitrace { file: String },
    #[serde(rename = "LANGFUSE_LEGACY")]
    LangfuseLegacy {
        #[serde(rename = "observationsFile")]
        observations_file: String,
        #[serde(rename = "tracesFile")]
        traces_file: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InputFileDto {
    pub role: InputRole,
    pub path: String,
    pub size_bytes: u64,
    pub modified_at: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasourceDto {
    pub id: String,
    pub source: DatasourceSource,
    pub inputs: Vec<InputFileDto>,
    pub state: &'static str,
    pub created_at: String,
    pub last_accessed_at: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct QueryRequest {
    pub sql: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ShutdownResponse {
    pub state: &'static str,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDatasetRequest {
    pub dataset: DatasetLocation,
    pub input: DatasetSourceInput,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatasetLocation {
    pub name: String,
    pub directory: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(tag = "source", deny_unknown_fields)]
pub enum DatasetSourceInput {
    #[serde(rename = "HITRACE")]
    Hitrace { file: String },
    #[serde(rename = "LANGFUSE_LEGACY")]
    LangfuseLegacy {
        #[serde(rename = "observationsFile")]
        observations_file: String,
        #[serde(rename = "tracesFile")]
        traces_file: String,
    },
    #[serde(rename = "SQLITE")]
    Sqlite { file: String },
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasetResponse {
    pub dataset: DatasetDto,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasetDto {
    pub name: String,
    pub directory: String,
    pub path: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasetInspectResponse {
    pub dataset: DatasetDto,
    pub tables: Vec<DatasetTableDto>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasetTableDto {
    pub kind: String,
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatasetQueryRequest {
    pub dataset: DatasetLocation,
    pub sql: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateRunRequest {
    pub pack_ref: String,
    pub dataset: DatasetLocation,
    pub inputs: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunSummaryDto {
    pub run_id: String,
    pub status: String,
    pub pack_ref: String,
    pub dataset: DatasetDto,
    pub step_count: usize,
    pub evidence_count: usize,
    pub brief_section_count: usize,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunDetailDto {
    pub summary: RunSummaryDto,
    pub steps: Vec<RunStepDto>,
    pub diagnostics: Vec<Value>,
    pub snapshot_digest: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunStepDto {
    pub id: String,
    pub uses: String,
    pub status: String,
    pub output: Option<String>,
    pub row_count: Option<usize>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunEvidenceResponse {
    pub run_id: String,
    pub evidence: Vec<Value>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunBriefResponse {
    pub run_id: String,
    pub sections: Vec<Value>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponse<M> {
    pub meta: M,
    pub row_count: usize,
    pub data: Vec<Value>,
}

impl<M> QueryResponse<M> {
    pub fn new(meta: M, data: Vec<Value>) -> Self {
        Self {
            row_count: data.len(),
            meta,
            data,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasourceQueryMeta {
    pub elapsed_ms: u128,
    pub datasource_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasetQueryMeta {
    pub elapsed_ms: u128,
    pub dataset: DatasetDto,
}
