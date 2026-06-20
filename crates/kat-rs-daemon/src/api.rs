use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct DataEnvelope<T> {
    pub data: T,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DataEnvelopeWithMeta<T, M> {
    pub data: T,
    pub meta: M,
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
#[serde(rename_all = "camelCase")]
pub struct QueryResponse {
    pub rows: Vec<Value>,
    pub row_count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryMeta {
    pub datasource_id: String,
    pub elapsed_ms: u128,
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

#[derive(Debug, Deserialize, Serialize, ToSchema)]
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
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasetResponse {
    pub dataset: DatasetDto,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasetDto {
    pub name: String,
    pub directory: String,
    pub path: String,
}
