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

/// One strictly typed scalar transported by the private nested-Run protocol.
///
/// In particular, `Int64` uses a canonical decimal string so JSON number
/// implementations cannot round an input at the signed 64-bit boundary.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub(crate) enum NestedScalar {
    String(String),
    Int64(String),
    Float64(f64),
    Boolean(bool),
    Duration(String),
    WallClockTimestamp(String),
    None,
}

#[derive(Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum RawNestedScalar {
    String(String),
    Int64(String),
    Float64(f64),
    Boolean(bool),
    Duration(String),
    WallClockTimestamp(String),
    None,
}

impl<'de> Deserialize<'de> for NestedScalar {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let scalar = RawNestedScalar::deserialize(deserializer)?;
        match scalar {
            RawNestedScalar::String(value) => Ok(Self::String(value)),
            RawNestedScalar::Int64(value) => {
                let canonical = value
                    .parse::<i64>()
                    .ok()
                    .is_some_and(|parsed| parsed.to_string() == value);
                if canonical {
                    Ok(Self::Int64(value))
                } else {
                    Err(serde::de::Error::custom(
                        "int64 value must be one canonical signed decimal string",
                    ))
                }
            }
            RawNestedScalar::Float64(value) if value.is_finite() => Ok(Self::Float64(value)),
            RawNestedScalar::Float64(_) => {
                Err(serde::de::Error::custom("float64 value must be finite"))
            }
            RawNestedScalar::Boolean(value) => Ok(Self::Boolean(value)),
            RawNestedScalar::Duration(value) => Ok(Self::Duration(value)),
            RawNestedScalar::WallClockTimestamp(value) => Ok(Self::WallClockTimestamp(value)),
            RawNestedScalar::None => Ok(Self::None),
        }
    }
}

/// A nested Workflow call after transport-only correlation data is removed.
#[derive(Debug)]
pub(crate) struct NestedRunCall {
    pub(crate) pack_name: String,
    pub(crate) workflow_name: String,
    pub(crate) inputs: BTreeMap<String, NestedScalar>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NestedRunRequestFrame {
    pub(super) call_id: u64,
    pack_name: String,
    workflow_name: String,
    inputs: BTreeMap<String, NestedScalar>,
}

impl NestedRunRequestFrame {
    pub(super) fn into_call(self) -> NestedRunCall {
        NestedRunCall {
            pack_name: self.pack_name,
            workflow_name: self.workflow_name,
            inputs: self.inputs,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NestedRelation {
    pub(crate) name: String,
    pub(crate) path: String,
}

/// Business result returned by a Rust nested-Run callback.
pub(crate) enum NestedRunOutcome {
    Success { relations: Vec<NestedRelation> },
    Failure { message: String },
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum NestedRunResponseFrame {
    Success {
        call_id: u64,
        relations: Vec<NestedRelation>,
    },
    Failure {
        call_id: u64,
        message: String,
    },
}

impl NestedRunResponseFrame {
    pub(super) fn from_outcome(call_id: u64, outcome: NestedRunOutcome) -> Self {
        match outcome {
            NestedRunOutcome::Success { relations } => Self::Success { call_id, relations },
            NestedRunOutcome::Failure { message } => Self::Failure { call_id, message },
        }
    }
}

/// One test-only control call after transport correlation is removed.
///
/// A `test_pack` Runtime is long-lived across pytest cases, so its Host grants
/// an opaque Session capability per test and a parent candidate capability per
/// `kat_run` call. Production `run_workflow` frames deliberately remain the
/// smaller untagged nested-Run shape above.
pub(crate) enum TestControlCall {
    BeginSession,
    BeginRun {
        test_session_id: String,
        pack_name: String,
        workflow_name: String,
    },
    RunWorkflow {
        test_run_id: String,
        call: NestedRunCall,
    },
    EndRun {
        test_run_id: String,
    },
    EndSession {
        test_session_id: String,
    },
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum TestControlRequestFrame {
    BeginTestSession {
        call_id: u64,
    },
    BeginTestRun {
        call_id: u64,
        test_session_id: String,
        pack_name: String,
        workflow_name: String,
    },
    RunWorkflow {
        call_id: u64,
        test_run_id: String,
        pack_name: String,
        workflow_name: String,
        inputs: BTreeMap<String, NestedScalar>,
    },
    EndTestRun {
        call_id: u64,
        test_run_id: String,
    },
    EndTestSession {
        call_id: u64,
        test_session_id: String,
    },
}

impl TestControlRequestFrame {
    pub(super) fn into_call(self) -> (u64, TestControlCall) {
        match self {
            Self::BeginTestSession { call_id } => (call_id, TestControlCall::BeginSession),
            Self::BeginTestRun {
                call_id,
                test_session_id,
                pack_name,
                workflow_name,
            } => (
                call_id,
                TestControlCall::BeginRun {
                    test_session_id,
                    pack_name,
                    workflow_name,
                },
            ),
            Self::RunWorkflow {
                call_id,
                test_run_id,
                pack_name,
                workflow_name,
                inputs,
            } => (
                call_id,
                TestControlCall::RunWorkflow {
                    test_run_id,
                    call: NestedRunCall {
                        pack_name,
                        workflow_name,
                        inputs,
                    },
                },
            ),
            Self::EndTestRun {
                call_id,
                test_run_id,
            } => (call_id, TestControlCall::EndRun { test_run_id }),
            Self::EndTestSession {
                call_id,
                test_session_id,
            } => (call_id, TestControlCall::EndSession { test_session_id }),
        }
    }
}

pub(crate) struct TestRunCapability {
    pub(crate) test_run_id: String,
    pub(crate) candidate_id: String,
    pub(crate) candidate_path: String,
    pub(crate) datasource_root: String,
    pub(crate) scratch_root: String,
}

pub(crate) enum TestControlOutcome {
    SessionStarted { test_session_id: String },
    RunStarted(TestRunCapability),
    Workflow(NestedRunOutcome),
    Complete,
    Failure { message: String },
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
pub(super) struct RunWorkflowWithInputsRequest<'a> {
    pub(super) operation: &'static str,
    pub(super) pack_name: &'a str,
    pub(super) pack_path: &'a str,
    pub(super) workflow_name: &'a str,
    pub(super) inputs: &'a BTreeMap<String, NestedScalar>,
    pub(super) candidate_id: &'a str,
    pub(super) candidate_path: &'a str,
    pub(super) datasource_root: &'a str,
    pub(super) scratch_root: &'a str,
}

#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum RunWorkflowRuntimeRequest<'a> {
    Arguments(RunWorkflowRequest<'a>),
    Inputs(RunWorkflowWithInputsRequest<'a>),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_run_request_is_strict_and_keeps_call_id_out_of_the_business_call() {
        let frame: NestedRunRequestFrame = serde_json::from_value(serde_json::json!({
            "call_id": 7,
            "pack_name": "source-pack",
            "workflow_name": "summarize",
            "inputs": {
                "limit": {"type": "int64", "value": "9223372036854775807"}
            }
        }))
        .unwrap();

        assert_eq!(frame.call_id, 7);
        let call = frame.into_call();
        assert_eq!(call.pack_name, "source-pack");
        assert_eq!(call.workflow_name, "summarize");
        assert!(matches!(
            call.inputs.get("limit"),
            Some(NestedScalar::Int64(value)) if value == "9223372036854775807"
        ));

        for invalid in [
            serde_json::json!({
                "call_id": 7,
                "pack_name": "source-pack",
                "workflow_name": "summarize",
                "inputs": {},
                "unexpected": true
            }),
            serde_json::json!({
                "call_id": 7,
                "pack_name": "source-pack",
                "inputs": {}
            }),
        ] {
            assert!(serde_json::from_value::<NestedRunRequestFrame>(invalid).is_err());
        }
    }

    #[test]
    fn typed_int64_uses_one_canonical_decimal_string() {
        let scalar = NestedScalar::Int64("-9223372036854775808".to_owned());
        assert_eq!(
            serde_json::to_value(&scalar).unwrap(),
            serde_json::json!({
                "type": "int64",
                "value": "-9223372036854775808"
            })
        );

        for invalid in ["+1", "01", "-0", "9223372036854775808", " 1"] {
            assert!(
                serde_json::from_value::<NestedScalar>(serde_json::json!({
                    "type": "int64",
                    "value": invalid
                }))
                .is_err(),
                "accepted non-canonical int64 {invalid:?}"
            );
        }
    }

    #[test]
    fn workflow_execution_requests_have_distinct_exact_input_shapes() {
        let arguments = vec!["--limit".to_owned(), "5".to_owned()];
        let cli = RunWorkflowRequest {
            operation: "run_workflow",
            pack_name: "source-pack",
            pack_path: "C:\\pack",
            workflow_name: "summarize",
            arguments: &arguments,
            candidate_id: "019f6e00-0000-7000-8000-000000000001",
            candidate_path: "C:\\session\\runs\\candidate",
            datasource_root: "C:\\session\\materializations",
            scratch_root: "C:\\session\\scratch\\candidate",
        };
        let nested_inputs =
            BTreeMap::from([("limit".to_owned(), NestedScalar::Int64("5".to_owned()))]);
        let nested = RunWorkflowWithInputsRequest {
            operation: "run_workflow_with_inputs",
            pack_name: "source-pack",
            pack_path: "C:\\pack",
            workflow_name: "summarize",
            inputs: &nested_inputs,
            candidate_id: "019f6e00-0000-7000-8000-000000000001",
            candidate_path: "C:\\session\\runs\\candidate",
            datasource_root: "C:\\session\\materializations",
            scratch_root: "C:\\session\\scratch\\candidate",
        };

        let cli = serde_json::to_value(cli).unwrap();
        let nested = serde_json::to_value(nested).unwrap();
        assert!(cli["arguments"].is_array());
        assert!(cli.get("inputs").is_none());
        assert_eq!(nested["operation"], "run_workflow_with_inputs");
        assert!(nested.get("arguments").is_none());
        assert!(nested["inputs"].is_object());
    }

    #[test]
    fn nested_response_frames_have_the_fixed_wire_shape() {
        let success = NestedRunResponseFrame::from_outcome(
            9,
            NestedRunOutcome::Success {
                relations: vec![NestedRelation {
                    name: "main".to_owned(),
                    path: "C:\\private\\main.parquet".to_owned(),
                }],
            },
        );
        assert_eq!(
            serde_json::to_value(success).unwrap(),
            serde_json::json!({
                "call_id": 9,
                "status": "success",
                "relations": [
                    {"name": "main", "path": "C:\\private\\main.parquet"}
                ]
            })
        );

        let failure = NestedRunResponseFrame::from_outcome(
            10,
            NestedRunOutcome::Failure {
                message: "nested Workflow failed".to_owned(),
            },
        );
        assert_eq!(
            serde_json::to_value(failure).unwrap(),
            serde_json::json!({
                "call_id": 10,
                "status": "failure",
                "message": "nested Workflow failed"
            })
        );
    }
}
