use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::response::KatDiagnostic;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Workflow {
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) required_tables: Vec<String>,
    pub(crate) parameters: Vec<Parameter>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Parameter {
    pub(crate) name: String,
    pub(crate) option: String,
    #[serde(rename = "type")]
    pub(crate) parameter_type: ParameterType,
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
pub(crate) enum ParameterType {
    String,
    Int64,
    Float64,
    Boolean,
    Duration,
    WallClockTimestamp,
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
pub(super) struct InspectPackRuntimeResult {
    pub(super) workflows: Vec<Workflow>,
}

#[derive(Serialize)]
pub(crate) struct ResolvedDatasetRequest {
    pub(crate) path: String,
    pub(crate) tables: BTreeMap<String, String>,
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
    pub(super) sql: &'a str,
}

#[derive(Serialize)]
pub(super) struct InspectPackRequest<'a> {
    pub(super) operation: &'static str,
    pub(super) pack_name: &'a str,
    pub(super) pack_path: &'a str,
}
