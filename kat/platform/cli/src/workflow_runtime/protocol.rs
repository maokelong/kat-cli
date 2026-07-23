use std::{collections::HashSet, sync::LazyLock};

use regex::Regex;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::response::KatDiagnostic;

use super::RuntimeInfrastructureError;

static WORKFLOW_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A[a-z0-9]+(?:-[a-z0-9]+)*\z").unwrap());
static TABLE_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A[a-z][a-z0-9]*(?:_[a-z0-9]+)*\z").unwrap());
static DURATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A(?<whole>[0-9]+)(?:\.(?<fraction>[0-9]{1,9}))?(?<unit>ns|us|ms|s|min|h)\z")
        .unwrap()
});

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
    Success {
        result: R,
    },
    Failure {
        failure_owner: RuntimeFailureOwner,
        error: KatDiagnostic,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RuntimeFailureOwner {
    RuntimeRequest,
    Pack,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InspectPackRuntimeResult {
    pub(super) workflows: Vec<Workflow>,
}

pub(super) fn validate_workflows(workflows: &[Workflow]) -> Result<(), RuntimeInfrastructureError> {
    let mut previous_name: Option<&str> = None;
    for workflow in workflows {
        if !WORKFLOW_NAME.is_match(&workflow.name) {
            return invalid_response(format!("invalid Workflow name {:?}", workflow.name));
        }
        normalized_non_empty(&workflow.title, "Workflow title")?;
        normalized_non_empty(&workflow.description, "Workflow description")?;
        if previous_name.is_some_and(|previous| previous >= workflow.name.as_str()) {
            return Err(RuntimeInfrastructureError::InvalidResponse(
                "Workflow names must be strictly sorted and unique".to_owned(),
            ));
        }
        previous_name = Some(&workflow.name);
        if !strictly_sorted_unique(&workflow.required_tables) {
            return Err(RuntimeInfrastructureError::InvalidResponse(
                "Required tables must be strictly sorted and unique".to_owned(),
            ));
        }
        for table in &workflow.required_tables {
            if !valid_table_name(table) {
                return invalid_response(format!("invalid Required table name {table:?}"));
            }
        }
        let mut parameter_names = HashSet::new();
        for parameter in &workflow.parameters {
            if !parameter_names.insert(&parameter.name) {
                return Err(RuntimeInfrastructureError::InvalidResponse(format!(
                    "duplicate Workflow parameter {:?}",
                    parameter.name
                )));
            }
            validate_parameter(parameter)?;
        }
    }
    Ok(())
}

fn validate_parameter(parameter: &Parameter) -> Result<(), RuntimeInfrastructureError> {
    non_empty(&parameter.name, "parameter name")?;
    normalized_non_empty(&parameter.description, "parameter description")?;
    let expected_option = format!("--{}", parameter.name.replace('_', "-"));
    if parameter.option != expected_option {
        return invalid_response(format!(
            "parameter {:?} must use option {expected_option:?}",
            parameter.name
        ));
    }
    let supported = [
        "string",
        "int64",
        "float64",
        "boolean",
        "duration",
        "wall_clock_timestamp",
    ];
    if !supported.contains(&parameter.parameter_type.as_str()) {
        return Err(RuntimeInfrastructureError::InvalidResponse(format!(
            "unsupported parameter type {:?}",
            parameter.parameter_type
        )));
    }
    if (parameter.parameter_type == "boolean") != parameter.negative_option.is_some() {
        return Err(RuntimeInfrastructureError::InvalidResponse(
            "only boolean parameters must contain negative_option".to_owned(),
        ));
    }
    if let Some(negative_option) = &parameter.negative_option {
        let expected_negative = format!("--no-{}", parameter.name.replace('_', "-"));
        if negative_option != &expected_negative {
            return invalid_response(format!(
                "boolean parameter {:?} must use negative option {expected_negative:?}",
                parameter.name
            ));
        }
    }
    if parameter.parameter_type == "boolean" && parameter.required {
        return invalid_response("boolean parameters require a default".to_owned());
    }
    if let Some(choices) = &parameter.choices
        && (parameter.parameter_type != "string"
            || choices.is_empty()
            || !strictly_sorted_unique(choices))
    {
        return Err(RuntimeInfrastructureError::InvalidResponse(
            "choices must be a non-empty sorted unique string set".to_owned(),
        ));
    }
    if parameter.required != parameter.default.is_missing() {
        return Err(RuntimeInfrastructureError::InvalidResponse(
            "required parameters omit default and optional parameters include it".to_owned(),
        ));
    }
    if let ParameterDefault::Value(default) = &parameter.default {
        let valid = match (parameter.parameter_type.as_str(), default) {
            (parameter_type, serde_json::Value::Null) => parameter_type != "boolean",
            ("boolean", serde_json::Value::Bool(_)) => true,
            ("float64", serde_json::Value::Number(number)) => {
                number.as_f64().is_some_and(f64::is_finite)
            }
            ("string", serde_json::Value::String(_)) => true,
            ("duration", serde_json::Value::String(value)) => valid_duration(value),
            ("wall_clock_timestamp", serde_json::Value::String(value)) => {
                valid_wall_clock_timestamp(value)
            }
            ("int64", serde_json::Value::String(value)) => value
                .parse::<i64>()
                .is_ok_and(|parsed| parsed.to_string() == *value),
            _ => false,
        };
        if !valid {
            return Err(RuntimeInfrastructureError::InvalidResponse(
                "parameter default does not match its public type".to_owned(),
            ));
        }
        if let (Some(choices), serde_json::Value::String(value)) = (&parameter.choices, default)
            && choices.binary_search(value).is_err()
        {
            return Err(RuntimeInfrastructureError::InvalidResponse(
                "string Literal default must be one of its choices".to_owned(),
            ));
        }
    }
    Ok(())
}

fn non_empty(value: &str, label: &str) -> Result<(), RuntimeInfrastructureError> {
    if value.trim().is_empty() {
        return Err(RuntimeInfrastructureError::InvalidResponse(format!(
            "{label} must not be empty"
        )));
    }
    Ok(())
}

fn normalized_non_empty(value: &str, label: &str) -> Result<(), RuntimeInfrastructureError> {
    non_empty(value, label)?;
    if value != value.trim() {
        return invalid_response(format!("{label} must not contain outer whitespace"));
    }
    Ok(())
}

fn invalid_response<T>(message: String) -> Result<T, RuntimeInfrastructureError> {
    Err(RuntimeInfrastructureError::InvalidResponse(message))
}

fn valid_table_name(value: &str) -> bool {
    TABLE_NAME.is_match(value)
        && !matches!(
            value,
            "con"
                | "prn"
                | "aux"
                | "nul"
                | "com1"
                | "com2"
                | "com3"
                | "com4"
                | "com5"
                | "com6"
                | "com7"
                | "com8"
                | "com9"
                | "lpt1"
                | "lpt2"
                | "lpt3"
                | "lpt4"
                | "lpt5"
                | "lpt6"
                | "lpt7"
                | "lpt8"
                | "lpt9"
        )
}

fn valid_duration(value: &str) -> bool {
    let Some(captures) = DURATION.captures(value) else {
        return false;
    };
    let whole_text = captures["whole"].trim_start_matches('0');
    let whole = if whole_text.is_empty() {
        0
    } else {
        let Some(whole) = whole_text.parse::<u128>().ok() else {
            return false;
        };
        whole
    };
    let factor = match &captures["unit"] {
        "ns" => 1_u128,
        "us" => 1_000,
        "ms" => 1_000_000,
        "s" => 1_000_000_000,
        "min" => 60_000_000_000,
        "h" => 3_600_000_000_000,
        _ => return false,
    };
    let Some(mut nanoseconds) = whole.checked_mul(factor) else {
        return false;
    };
    if let Some(fraction) = captures.name("fraction") {
        let denominator = 10_u128.pow(fraction.as_str().len() as u32);
        let Some(scaled_fraction) = fraction
            .as_str()
            .parse::<u128>()
            .ok()
            .and_then(|fraction| fraction.checked_mul(factor))
        else {
            return false;
        };
        if scaled_fraction % denominator != 0 {
            return false;
        }
        let Some(total) = nanoseconds.checked_add(scaled_fraction / denominator) else {
            return false;
        };
        nanoseconds = total;
    }
    nanoseconds <= i64::MAX as u128
}

fn valid_wall_clock_timestamp(value: &str) -> bool {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .and_then(|timestamp| timestamp.format(&Rfc3339).ok())
        .is_some_and(|canonical| canonical == value)
}

fn strictly_sorted_unique(values: &[String]) -> bool {
    values.windows(2).all(|items| items[0] < items[1])
}
