use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::response::KatDiagnostic;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Workflow {
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) parameters: Vec<Parameter>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    pub(crate) name: String,
    pub(crate) parameters: Vec<SourceParameter>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Parameter {
    pub(crate) name: String,
    pub(crate) option: String,
    #[serde(rename = "type")]
    pub(crate) parameter_type: WorkflowParameterType,
    pub(crate) required: bool,
    pub(crate) description: String,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub(crate) negative_option: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_strings")]
    pub(crate) choices: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_default")]
    pub(crate) default: ParameterDefault,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkflowParameterType {
    String,
    Int64,
    Float64,
    Boolean,
    Duration,
    WallClockTimestamp,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceParameter {
    pub(crate) name: String,
    pub(crate) option: String,
    #[serde(rename = "type")]
    pub(crate) parameter_type: SourceParameterType,
    pub(crate) required: bool,
    #[serde(default)]
    pub(crate) repeatable: bool,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub(crate) negative_option: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_strings")]
    pub(crate) choices: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_source_default")]
    pub(crate) default: SourceParameterDefault,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceParameterType {
    String,
    Int64,
    Float64,
    Boolean,
    Duration,
    WallClockTimestamp,
    Path,
}

#[derive(Default)]
pub(crate) enum ParameterDefault {
    #[default]
    Missing,
    Value(JsonScalar),
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum JsonScalar {
    String(String),
    Number(serde_json::Number),
    Boolean(bool),
    Null(()),
}

impl ParameterDefault {
    pub(crate) fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
}

impl Serialize for ParameterDefault {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Missing => serializer.serialize_none(),
            Self::Value(value) => value.serialize(serializer),
        }
    }
}

fn deserialize_default<'de, D>(deserializer: D) -> Result<ParameterDefault, D::Error>
where
    D: serde::Deserializer<'de>,
{
    JsonScalar::deserialize(deserializer).map(ParameterDefault::Value)
}

#[derive(Default)]
pub(crate) enum SourceParameterDefault {
    #[default]
    Missing,
    Value(SourceDefaultValue),
}

pub(crate) enum SourceDefaultValue {
    Scalar(JsonScalar),
    Paths(Vec<String>),
}

impl SourceParameterDefault {
    pub(crate) fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
}

impl Serialize for SourceParameterDefault {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Missing => serializer.serialize_none(),
            Self::Value(SourceDefaultValue::Scalar(value)) => value.serialize(serializer),
            Self::Value(SourceDefaultValue::Paths(value)) => value.serialize(serializer),
        }
    }
}

fn deserialize_source_default<'de, D>(deserializer: D) -> Result<SourceParameterDefault, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    let value = serde_json::Value::deserialize(deserializer)?;
    let value = match value {
        serde_json::Value::String(value) => SourceDefaultValue::Scalar(JsonScalar::String(value)),
        serde_json::Value::Number(value) => SourceDefaultValue::Scalar(JsonScalar::Number(value)),
        serde_json::Value::Bool(value) => SourceDefaultValue::Scalar(JsonScalar::Boolean(value)),
        serde_json::Value::Null => SourceDefaultValue::Scalar(JsonScalar::Null(())),
        serde_json::Value::Array(values) => SourceDefaultValue::Paths(
            values
                .into_iter()
                .map(|value| match value {
                    serde_json::Value::String(value) => Ok(value),
                    _ => Err(D::Error::custom(
                        "repeated Source Path default must contain only strings",
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        serde_json::Value::Object(_) => {
            return Err(D::Error::custom(
                "Source parameter default must be a scalar or path array",
            ));
        }
    };
    Ok(SourceParameterDefault::Value(value))
}

fn deserialize_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}

fn deserialize_optional_strings<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<String>::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum RuntimeResponse<R> {
    Success { result: R },
    Failure { error: KatDiagnostic },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectPackRuntimeResult {
    pub(crate) source_guide: Option<String>,
    pub(crate) sources: Vec<Source>,
    pub(crate) workflows: Vec<Workflow>,
}

#[derive(Serialize)]
pub(crate) struct ResolvedDatasetRequest {
    pub(crate) path: String,
    pub(crate) sources: Vec<ResolvedSourceRequest>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ResolvedSourceRequest {
    External {
        pack: String,
        source: String,
        arguments: Vec<String>,
        working_directory: String,
    },
    Materialized {
        pack: String,
        source: String,
        tables: Vec<ResolvedTableRequest>,
    },
}

#[derive(Serialize)]
pub(crate) struct ResolvedTableRequest {
    pub(crate) name: String,
    pub(crate) path: String,
}

#[derive(Serialize)]
pub(crate) struct QueryPackSearchRequest {
    pub(crate) candidates: BTreeMap<String, Vec<String>>,
    pub(crate) issues: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawRunWorkflowResult {
    pub(super) effective_inputs: BTreeMap<String, serde_json::Value>,
    pub(super) outputs: BTreeMap<String, RawRuntimeOutput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawRuntimeOutput {
    pub(super) columns: Vec<Column>,
    pub(super) row_count: u64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Column {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) data_type: String,
}

#[derive(Serialize)]
pub(super) struct RunWorkflowRequest<'a> {
    pub(super) operation: &'static str,
    pub(super) pack_name: &'a str,
    pub(super) pack_path: &'a str,
    pub(super) pack_paths: &'a BTreeMap<String, String>,
    pub(super) workflow_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) dataset: Option<&'a ResolvedDatasetRequest>,
    pub(super) arguments: &'a [String],
    pub(super) candidate_id: &'a str,
    pub(super) candidate_path: &'a str,
}

#[derive(Serialize)]
pub(super) struct QueryRunRequest<'a> {
    pub(super) operation: &'static str,
    pub(super) run_path: &'a str,
    pub(super) outputs: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) dataset: Option<&'a ResolvedDatasetRequest>,
    pub(super) pack_search: &'a QueryPackSearchRequest,
    pub(super) sql: &'a str,
}

#[derive(Serialize)]
pub(super) struct QueryDatasetRequest<'a> {
    pub(super) operation: &'static str,
    pub(super) dataset: &'a ResolvedDatasetRequest,
    pub(super) pack_search: &'a QueryPackSearchRequest,
    pub(super) sql: &'a str,
}

#[derive(Serialize)]
pub(super) struct InspectPackRequest<'a> {
    pub(super) operation: &'static str,
    pub(super) pack_name: &'a str,
    pub(super) pack_path: &'a str,
}

#[derive(Serialize)]
pub(super) struct BindSourceRequest<'a> {
    pub(super) operation: &'static str,
    pub(super) pack_name: &'a str,
    pub(super) pack_path: &'a str,
    pub(super) source_name: &'a str,
    pub(super) arguments: &'a [String],
    pub(super) argument_base: &'a str,
}

#[derive(Serialize)]
pub(super) struct MaterializeSourceRequest<'a> {
    pub(super) operation: &'static str,
    pub(super) pack_name: &'a str,
    pub(super) pack_path: &'a str,
    pub(super) source_name: &'a str,
    pub(super) arguments: &'a [String],
    pub(super) argument_base: &'a str,
    pub(super) tables: &'a [String],
    pub(super) export_path: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BindSourceResult {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MaterializeSourceResult {
    pub(crate) tables: Vec<String>,
}

#[derive(Serialize)]
pub(super) struct TestPackRequest<'a> {
    pub(super) operation: &'static str,
    pub(super) pack_name: &'a str,
    pub(super) pack_path: &'a str,
    pub(super) datasets: &'a BTreeMap<String, ResolvedDatasetRequest>,
    pub(super) tests: &'a [String],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TestPackResult {
    pub(crate) summary: BTreeMap<String, u64>,
}
