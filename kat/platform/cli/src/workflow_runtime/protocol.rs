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
    pub(crate) parameter_type: String,
    pub(crate) required: bool,
    pub(crate) description: String,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub(crate) negative_option: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_strings")]
    pub(crate) choices: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_default")]
    pub(crate) default: ParameterDefault,
}

#[derive(Default)]
pub(crate) enum ParameterDefault {
    #[default]
    Missing,
    Value(serde_json::Value),
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
    serde_json::Value::deserialize(deserializer).map(ParameterDefault::Value)
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
