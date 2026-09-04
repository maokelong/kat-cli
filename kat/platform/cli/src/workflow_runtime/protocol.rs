use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::response::KatDiagnostic;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectionSummary {
    pub(crate) name: String,
    pub(crate) description: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkflowInspection {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) parameters: Vec<Parameter>,
    #[serde(deserialize_with = "deserialize_nullable_string")]
    pub(crate) guide: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderInspection {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) module: String,
    pub(crate) qualname: String,
    pub(crate) guide: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Parameter {
    pub(crate) name: String,
    pub(crate) option: String,
    #[serde(rename = "type")]
    pub(crate) parameter_type: ParameterType,
    pub(crate) required: bool,
    pub(crate) description: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) negative_option: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_strings",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) choices: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_default",
        skip_serializing_if = "ParameterDefault::is_missing"
    )]
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

fn deserialize_nullable_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum RuntimeResponse<R> {
    Success { result: R },
    Failure { error: KatDiagnostic },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectWorkflowsResult {
    pub(crate) workflows: Vec<InspectionSummary>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectWorkflowResult {
    pub(crate) workflow: WorkflowInspection,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectProvidersResult {
    pub(crate) providers: Vec<InspectionSummary>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectProviderResult {
    pub(crate) provider: ProviderInspection,
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
    pub(super) arguments: &'a [String],
    pub(super) candidate_id: &'a str,
    pub(super) candidate_path: &'a str,
    pub(super) datasource_root: &'a str,
    pub(super) scratch_root: &'a str,
}

#[derive(Serialize)]
pub(super) struct QueryRunRequest<'a> {
    pub(super) operation: &'static str,
    pub(super) outputs: &'a BTreeMap<String, String>,
    pub(super) sql: &'a str,
    pub(super) result_path: &'a str,
}

#[derive(Serialize)]
pub(super) struct InspectWorkflowRequest<'a> {
    pub(super) operation: &'static str,
    pub(super) pack_name: &'a str,
    pub(super) pack_path: &'a str,
    pub(super) workflow_name: Option<&'a str>,
}

#[derive(Serialize)]
pub(super) struct InspectProviderRequest<'a> {
    pub(super) operation: &'static str,
    pub(super) pack_name: &'a str,
    pub(super) pack_path: &'a str,
    pub(super) provider_name: Option<&'a str>,
}

#[derive(Serialize)]
pub(super) struct TestPackRequest<'a> {
    pub(super) operation: &'static str,
    pub(super) pack_name: &'a str,
    pub(super) pack_path: &'a str,
    pub(super) tests: &'a [String],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TestPackResult {
    pub(crate) summary: BTreeMap<String, u64>,
}
