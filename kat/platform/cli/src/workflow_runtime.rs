use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use miette::Diagnostic;
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::{
    operation_log::{OperationLog, OperationLogError},
    response::KatDiagnostic,
    text_projection::project_inline_text,
};

mod output_spool;
mod protocol;

use output_spool::RuntimeOutputSpool;
pub(crate) use protocol::{
    Column, ParameterDefault, ParameterType, ResolvedDatasetRequest, Workflow,
};
use protocol::{
    InspectPackRequest, InspectPackRuntimeResult, RawRunWorkflowResult, RunWorkflowRequest,
    RuntimeResponse,
};

const PRIVATE_RUNTIME_MODULE: &str = "_kat_runtime";

pub(crate) enum RuntimeOutcome<T> {
    Success {
        result: T,
        log_path: String,
    },
    Failure {
        diagnostic: KatDiagnostic,
        log_path: String,
    },
}

pub(crate) type InspectPackOutcome = RuntimeOutcome<Vec<Workflow>>;

/// Runtime execution outcome before the CLI performs the Run publication gate.
pub(crate) enum RunWorkflowOutcome {
    Success {
        result: RunWorkflowReport,
        log: OperationLog,
    },
    Failure {
        diagnostic: KatDiagnostic,
        log_path: String,
    },
}

/// Runtime-reported facts that passed the control protocol.
///
/// This report is suitable for Manifest assembly. The Workflow Runtime owns
/// Output materialization; its successful Response is the authority that the
/// named Outputs were written.
#[derive(Serialize)]
pub(crate) struct RunWorkflowReport {
    pub(crate) effective_inputs: BTreeMap<String, serde_json::Value>,
    pub(crate) outputs: BTreeMap<String, RunOutputMetadata>,
}

/// Runtime-reported metadata for one named Run Output.
#[derive(Clone, Serialize)]
pub(crate) struct RunOutputMetadata {
    pub(crate) columns: Vec<Column>,
    pub(crate) row_count: u64,
}

pub(crate) struct RunWorkflowInvocation {
    pub(crate) pack_name: String,
    pub(crate) pack_path: String,
    pub(crate) workflow_name: String,
    pub(crate) dataset: Option<ResolvedDatasetRequest>,
    pub(crate) arguments: Vec<String>,
    pub(crate) candidate_id: String,
    pub(crate) candidate_path: String,
}

pub(crate) fn inspect_pack(
    mut log: OperationLog,
    pack_name: &str,
    pack_path: &Path,
) -> Result<InspectPackOutcome, InspectPackInfrastructureError> {
    let response = match exchange(pack_name, pack_path, &mut log) {
        Ok(response) => response,
        Err(ExchangeError::Log(error)) => {
            return Err(InspectPackInfrastructureError::operation_log(error));
        }
        Err(ExchangeError::Runtime(error)) => return Err(finish_runtime_error(log, error)),
        Err(ExchangeError::InvalidResponse(details)) => {
            return Err(finish_invalid_runtime_response(log, &details));
        }
    };
    match response {
        RuntimeResponse::Success { result } => {
            if let Err(source) = log.append(b"status: success\n") {
                return Err(InspectPackInfrastructureError::operation_log(source));
            }
            let log_path = log
                .finish()
                .map_err(InspectPackInfrastructureError::operation_log)?;
            Ok(InspectPackOutcome::Success {
                result: result.workflows,
                log_path,
            })
        }
        RuntimeResponse::Failure { error } => {
            if !error.validate() {
                return Err(finish_runtime_error(
                    log,
                    RuntimeInfrastructureError::InvalidResponse,
                ));
            }
            if let Err(source) = log.append(b"status: failure\n") {
                return Err(InspectPackInfrastructureError::operation_log(source));
            }
            let log_path = log
                .finish()
                .map_err(InspectPackInfrastructureError::operation_log)?;
            Ok(InspectPackOutcome::Failure {
                diagnostic: error,
                log_path,
            })
        }
    }
}

pub(crate) fn execute_workflow_runtime(
    mut log: OperationLog,
    invocation: RunWorkflowInvocation,
) -> Result<RunWorkflowOutcome, RunWorkflowError> {
    let request = RunWorkflowRequest {
        operation: "run_workflow",
        pack_name: &invocation.pack_name,
        pack_path: &invocation.pack_path,
        workflow_name: &invocation.workflow_name,
        dataset: invocation.dataset.as_ref(),
        arguments: &invocation.arguments,
        candidate_id: &invocation.candidate_id,
        candidate_path: &invocation.candidate_path,
    };
    let response = match exchange_request_bytes("kat-run-workflow-", &request, &mut log) {
        Ok(response) => response,
        Err(ExchangeError::Log(error)) => return Err(RunWorkflowError::operation_log(error)),
        Err(ExchangeError::Runtime(error)) => {
            return Err(finish_runtime_error(log, error));
        }
        Err(ExchangeError::InvalidResponse(details)) => {
            return Err(finish_invalid_runtime_response(log, &details));
        }
    };
    let response = match decode_and_validate_run_workflow_response(&response, &invocation) {
        Ok(response) => response,
        Err(violation) => {
            return Err(finish_invalid_runtime_response(log, &violation.details));
        }
    };
    match response {
        RuntimeResponse::Success { result } => {
            if let Err(source) = log.append(b"runtime_status: success\n") {
                return Err(RunWorkflowError::operation_log(source));
            }
            Ok(RunWorkflowOutcome::Success { result, log })
        }
        RuntimeResponse::Failure { error } => {
            if let Err(source) = log.append(b"runtime_status: failure\n") {
                return Err(RunWorkflowError::operation_log(source));
            }
            let log_path = log.finish().map_err(RunWorkflowError::operation_log)?;
            Ok(RunWorkflowOutcome::Failure {
                diagnostic: error,
                log_path,
            })
        }
    }
}

struct RuntimeResponseViolation {
    details: String,
}

fn decode_and_validate_run_workflow_response(
    response: &[u8],
    invocation: &RunWorkflowInvocation,
) -> Result<RuntimeResponse<RunWorkflowReport>, RuntimeResponseViolation> {
    let response: RuntimeResponse<RawRunWorkflowResult> = serde_json::from_slice(response)
        .map_err(|source| RuntimeResponseViolation {
            details: format!("Runtime Response decoding failed: {source}"),
        })?;
    match response {
        RuntimeResponse::Success { result } => validate_run_workflow_report(result, invocation)
            .map(|result| RuntimeResponse::Success { result }),
        RuntimeResponse::Failure { error } => {
            if !error.validate() || exposes_run_private_value(&error, invocation) {
                Err(RuntimeResponseViolation {
                    details: "Runtime Diagnostic contains invalid or private fields".to_owned(),
                })
            } else {
                Ok(RuntimeResponse::Failure { error })
            }
        }
    }
}

fn exposes_run_private_value(
    diagnostic: &KatDiagnostic,
    invocation: &RunWorkflowInvocation,
) -> bool {
    let values = [
        invocation.candidate_id.clone(),
        invocation.candidate_path.clone(),
        invocation.candidate_path.replace('\\', "/"),
    ];
    values
        .iter()
        .any(|value| diagnostic.contains_private_value(value))
}

fn validate_run_workflow_report(
    result: RawRunWorkflowResult,
    invocation: &RunWorkflowInvocation,
) -> Result<RunWorkflowReport, RuntimeResponseViolation> {
    if result_violates_private_value_isolation(&result, invocation) {
        return Err(RuntimeResponseViolation {
            details: "run_workflow success exposes a private Runtime value".to_owned(),
        });
    }
    if result.outputs.is_empty() {
        return Err(RuntimeResponseViolation {
            details: "run_workflow outputs must not be empty".to_owned(),
        });
    }
    for (name, value) in &result.effective_inputs {
        if name.is_empty()
            || !matches!(
                value,
                serde_json::Value::Null
                    | serde_json::Value::Bool(_)
                    | serde_json::Value::String(_)
                    | serde_json::Value::Number(_)
            )
        {
            return Err(RuntimeResponseViolation {
                details: format!("run_workflow effective input {name:?} has an invalid value"),
            });
        }
        if value.as_f64().is_some_and(|number| !number.is_finite()) {
            return Err(RuntimeResponseViolation {
                details: format!("run_workflow effective input {name:?} is not finite"),
            });
        }
    }
    for (name, output) in &result.outputs {
        if !valid_output_name(name) {
            return Err(RuntimeResponseViolation {
                details: format!("invalid Output name {name:?}"),
            });
        }
        for column in &output.columns {
            if column.name.is_empty() || column.data_type.trim().is_empty() {
                return Err(RuntimeResponseViolation {
                    details: format!("Output {name:?} has an empty column name or type"),
                });
            }
        }
    }
    Ok(RunWorkflowReport {
        effective_inputs: result.effective_inputs,
        outputs: result
            .outputs
            .into_iter()
            .map(|(name, output)| {
                (
                    name,
                    RunOutputMetadata {
                        columns: output.columns,
                        row_count: output.row_count,
                    },
                )
            })
            .collect(),
    })
}

fn result_violates_private_value_isolation(
    result: &RawRunWorkflowResult,
    invocation: &RunWorkflowInvocation,
) -> bool {
    let private_values = [
        invocation.candidate_id.as_str(),
        invocation.candidate_path.as_str(),
    ];
    let normalized_candidate_path = invocation.candidate_path.replace('\\', "/");
    let contains_private = |value: &str| {
        private_values.iter().any(|private| value.contains(private))
            || value.contains(&normalized_candidate_path)
    };

    result.effective_inputs.iter().any(|(name, value)| {
        contains_private(name) || value.as_str().is_some_and(&contains_private)
    }) || result.outputs.iter().any(|(name, output)| {
        contains_private(name)
            || output
                .columns
                .iter()
                .any(|column| contains_private(&column.name) || contains_private(&column.data_type))
    })
}

fn valid_output_name(name: &str) -> bool {
    !name.is_empty()
        && name.as_bytes()[0].is_ascii_lowercase()
        && name.split('_').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
        && !is_windows_device_name(name)
}

fn is_windows_device_name(name: &str) -> bool {
    matches!(name, "con" | "prn" | "aux" | "nul")
        || matches!(
            name.as_bytes(),
            [b'c', b'o', b'm', b'1'..=b'9'] | [b'l', b'p', b't', b'1'..=b'9']
        )
}

fn exchange(
    pack_name: &str,
    pack_path: &Path,
    log: &mut OperationLog,
) -> Result<RuntimeResponse<InspectPackRuntimeResult>, ExchangeError> {
    let pack_path_text = pack_path
        .to_str()
        .ok_or_else(|| RuntimeInfrastructureError::NonUnicodePackPath(pack_path.to_path_buf()))?;
    let request = InspectPackRequest {
        operation: "inspect_pack",
        pack_name,
        pack_path: pack_path_text,
    };
    exchange_request("kat-inspect-pack-", &request, log)
}

fn exchange_request<R: DeserializeOwned>(
    prefix: &str,
    request: &impl Serialize,
    log: &mut OperationLog,
) -> Result<RuntimeResponse<R>, ExchangeError> {
    let response = exchange_request_bytes(prefix, request, log)?;
    serde_json::from_slice(&response).map_err(|source| {
        ExchangeError::InvalidResponse(format!("Runtime Response decoding failed: {source}"))
    })
}

fn exchange_request_bytes(
    prefix: &str,
    request: &impl Serialize,
    log: &mut OperationLog,
) -> Result<Vec<u8>, ExchangeError> {
    let control = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .map_err(RuntimeInfrastructureError::ControlDirectory)?;
    let control_path =
        dunce::canonicalize(control.path()).map_err(RuntimeInfrastructureError::ControlPath)?;
    let request_path = control_path.join("request.json");
    let response_path = control_path.join("response.json");
    let output = RuntimeOutputSpool::create(&control_path)?;
    let (stdout, stderr) = output.stdio()?;
    let request = serde_json::to_vec(request).map_err(RuntimeInfrastructureError::EncodeRequest)?;
    fs::write(&request_path, request).map_err(RuntimeInfrastructureError::WriteRequest)?;

    let python = bundled_python_path()?;
    if !python.is_file() {
        return Err(RuntimeInfrastructureError::MissingHost(python).into());
    }
    let mut command = Command::new(&python);
    command
        .args(["-I", "-B", "-X", "utf8", "-u", "-m", PRIVATE_RUNTIME_MODULE])
        .arg("--request")
        .arg(&request_path)
        .arg("--response")
        .arg(&response_path)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr);
    let mut child = command
        .spawn()
        .map_err(|source| RuntimeInfrastructureError::StartHost { python, source })?;

    let status = match child.wait() {
        Ok(status) => status,
        Err(source) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RuntimeInfrastructureError::WaitHost(source).into());
        }
    };
    output.append_to(log)?;
    if !status.success() {
        return Err(RuntimeInfrastructureError::HostExit(status.code()).into());
    }
    fs::read(&response_path)
        .map_err(RuntimeInfrastructureError::ReadResponse)
        .map_err(Into::into)
}

fn bundled_python_path() -> Result<PathBuf, RuntimeInfrastructureError> {
    let executable =
        std::env::current_exe().map_err(RuntimeInfrastructureError::CurrentExecutable)?;
    let payload = executable
        .parent()
        .ok_or_else(|| RuntimeInfrastructureError::InvalidPayload(executable.clone()))?;
    #[cfg(windows)]
    let python = payload.join("python").join("python.exe");
    #[cfg(not(windows))]
    let python = payload.join("python").join("bin").join("python3");
    Ok(python)
}

trait RuntimeOperationError: Sized {
    fn operation_log(source: OperationLogError) -> Self;
    fn runtime(source: RuntimeInfrastructureError, log_path: String) -> Self;
}

fn finish_runtime_error<E: RuntimeOperationError>(
    log: OperationLog,
    error: RuntimeInfrastructureError,
) -> E {
    let details = error.to_string();
    finish_runtime_error_with_details(log, error, &details)
}

fn finish_invalid_runtime_response<E: RuntimeOperationError>(
    log: OperationLog,
    details: &str,
) -> E {
    finish_runtime_error_with_details(log, RuntimeInfrastructureError::InvalidResponse, details)
}

fn finish_runtime_error_with_details<E: RuntimeOperationError>(
    mut log: OperationLog,
    error: RuntimeInfrastructureError,
    details: &str,
) -> E {
    let details = runtime_failure_log_details(details);
    if let Err(log_error) = log.append(details.as_bytes()) {
        return E::operation_log(log_error);
    }
    match log.finish() {
        Ok(log_path) => E::runtime(error, log_path),
        Err(log_error) => E::operation_log(log_error),
    }
}

fn runtime_failure_log_details(details: &str) -> String {
    format!("status: failure\nerror: {}\n", project_inline_text(details))
}

#[derive(Debug)]
enum ExchangeError {
    Log(OperationLogError),
    Runtime(RuntimeInfrastructureError),
    InvalidResponse(String),
}

impl From<RuntimeInfrastructureError> for ExchangeError {
    fn from(error: RuntimeInfrastructureError) -> Self {
        Self::Runtime(error)
    }
}

#[derive(Debug, Error, Diagnostic)]
pub(crate) enum InspectPackInfrastructureError {
    #[error("PACK inspection Operation log could not be delivered")]
    #[diagnostic(help("Provide a writable KAT Data Home and retry the complete inspection"))]
    OperationLog {
        #[source]
        source: OperationLogError,
        log_path: Option<String>,
    },
    #[error("PACK inspection Operation log is incomplete")]
    #[diagnostic(help(
        "Inspect the partial log if present, then provide writable storage and retry"
    ))]
    IncompleteOperationLog {
        #[source]
        source: OperationLogError,
        log_path: String,
    },
    #[error("PACK inspection Runtime failed")]
    #[diagnostic(help("Inspect the Operation log, correct the PACK or deployment, and retry"))]
    Runtime {
        #[source]
        source: RuntimeInfrastructureError,
        log_path: String,
    },
}

impl InspectPackInfrastructureError {
    fn operation_log(source: OperationLogError) -> Self {
        match source.readable_path() {
            Some(log_path) => Self::IncompleteOperationLog { source, log_path },
            None => Self::OperationLog {
                source,
                log_path: None,
            },
        }
    }

    pub(crate) fn log_path(&self) -> Option<String> {
        match self {
            Self::OperationLog { log_path, .. } => log_path.clone(),
            Self::IncompleteOperationLog { log_path, .. } => Some(log_path.clone()),
            Self::Runtime { log_path, .. } => Some(log_path.clone()),
        }
    }
}

impl RuntimeOperationError for InspectPackInfrastructureError {
    fn operation_log(source: OperationLogError) -> Self {
        Self::operation_log(source)
    }

    fn runtime(source: RuntimeInfrastructureError, log_path: String) -> Self {
        Self::Runtime { source, log_path }
    }
}

#[derive(Debug, Error, Diagnostic)]
pub(crate) enum RunWorkflowError {
    #[error("Run Operation log could not be delivered")]
    #[diagnostic(help("Provide a writable KAT Data Home and retry the complete Run"))]
    OperationLog {
        #[source]
        source: OperationLogError,
        log_path: Option<String>,
    },
    #[error("Run Operation log is incomplete")]
    #[diagnostic(help(
        "Inspect the partial log if present, then provide writable storage and retry"
    ))]
    IncompleteOperationLog {
        #[source]
        source: OperationLogError,
        log_path: String,
    },
    #[error("Workflow Runtime failed")]
    #[diagnostic(help("Inspect the Operation log, correct the inputs or deployment, and retry"))]
    Runtime {
        #[source]
        source: RuntimeInfrastructureError,
        log_path: String,
    },
}

impl RunWorkflowError {
    fn operation_log(source: OperationLogError) -> Self {
        match source.readable_path() {
            Some(log_path) => Self::IncompleteOperationLog { source, log_path },
            None => Self::OperationLog {
                source,
                log_path: None,
            },
        }
    }

    pub(crate) fn log_path(&self) -> Option<String> {
        match self {
            Self::OperationLog { log_path, .. } => log_path.clone(),
            Self::IncompleteOperationLog { log_path, .. } => Some(log_path.clone()),
            Self::Runtime { log_path, .. } => Some(log_path.clone()),
        }
    }
}

impl RuntimeOperationError for RunWorkflowError {
    fn operation_log(source: OperationLogError) -> Self {
        Self::operation_log(source)
    }

    fn runtime(source: RuntimeInfrastructureError, log_path: String) -> Self {
        Self::Runtime { source, log_path }
    }
}

#[derive(Debug, Error)]
pub(crate) enum RuntimeInfrastructureError {
    #[error("PACK path cannot be represented as native Unicode: {0:?}")]
    NonUnicodePackPath(PathBuf),
    #[error("failed to create Runtime control directory")]
    ControlDirectory(#[source] io::Error),
    #[error("failed to resolve Runtime control directory")]
    ControlPath(#[source] io::Error),
    #[error("failed to encode Runtime Request")]
    EncodeRequest(#[source] serde_json::Error),
    #[error("failed to write Runtime Request")]
    WriteRequest(#[source] io::Error),
    #[error("failed to locate the current KAT executable")]
    CurrentExecutable(#[source] io::Error),
    #[error("KAT executable has no Platform Payload directory: {0}")]
    InvalidPayload(PathBuf),
    #[error("Bundled Python Host is missing: {0}")]
    MissingHost(PathBuf),
    #[error("failed to start Bundled Python Host {python}")]
    StartHost {
        python: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to create Runtime output spool")]
    CreateOutputSpool(#[source] io::Error),
    #[error("failed to duplicate Runtime {stream} output spool")]
    CloneOutputSpool {
        stream: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("failed to read Runtime {stream} output spool")]
    ReadOutputSpool {
        stream: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("failed to wait for Bundled Python Host")]
    WaitHost(#[source] io::Error),
    #[error("Bundled Python Host exited without completing Runtime IPC (exit code {0:?})")]
    HostExit(Option<i32>),
    #[error("failed to read Runtime Response")]
    ReadResponse(#[source] io::Error),
    #[error("Runtime Response is not valid for the requested operation")]
    InvalidResponse,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn runtime_response_is_structurally_closed() {
        let valid_failure =
            br#"{"status":"failure","error":{"message":"Runtime Request is invalid"}}"#;
        assert!(matches!(
            serde_json::from_slice::<RuntimeResponse<InspectPackRuntimeResult>>(valid_failure)
                .unwrap(),
            RuntimeResponse::Failure { .. }
        ));

        for invalid in [
            br#"{"status":"success","result":{"workflows":[],"extra":true}}"#.as_slice(),
            br#"{"status":"failure","failure_owner":"pack","error":{"message":"failed"}}"#
                .as_slice(),
            br#"{"status":"failure","error":{"message":"failed"},"extra":true}"#.as_slice(),
            br#"{"status":"success","result":{"workflows":[{"name":"w","title":"W","description":"W.","required_tables":[],"parameters":[{"name":"value","option":"--value","type":"string","required":false,"description":"Value","default":[] }]}]}}"#.as_slice(),
            br#"{"status":"success","result":{"workflows":[{"name":"w","title":"W","description":"W.","required_tables":[],"parameters":[{"name":"value","option":"--value","type":"string","required":false,"description":"Value","default":{} }]}]}}"#.as_slice(),
            br#"{"status":"success","result":{"workflows":[{"name":"w","title":"W","description":"W.","required_tables":[],"parameters":[{"name":"value","option":"--value","type":"path","required":true,"description":"Value"}]}]}}"#.as_slice(),
        ] {
            assert!(
                serde_json::from_slice::<RuntimeResponse<InspectPackRuntimeResult>>(invalid)
                    .is_err()
            );
        }
    }

    #[test]
    fn runtime_response_accepts_only_closed_parameter_types() {
        for parameter_type in [
            "string",
            "int64",
            "float64",
            "boolean",
            "duration",
            "wall_clock_timestamp",
        ] {
            let response = format!(
                r#"{{"status":"success","result":{{"workflows":[{{"name":"w","title":"W","description":"W.","required_tables":[],"parameters":[{{"name":"value","option":"--value","type":"{parameter_type}","required":true,"description":"Value"}}]}}]}}}}"#
            );
            assert!(
                serde_json::from_str::<RuntimeResponse<InspectPackRuntimeResult>>(&response)
                    .is_ok(),
                "parameter type should be part of the closed set: {parameter_type}"
            );
        }
    }

    #[test]
    fn runtime_response_accepts_only_scalar_parameter_defaults() {
        for default in [r#""value""#, "42", "1.5", "true", "null"] {
            let response = format!(
                r#"{{"status":"success","result":{{"workflows":[{{"name":"w","title":"W","description":"W.","required_tables":[],"parameters":[{{"name":"value","option":"--value","type":"string","required":false,"description":"Value","default":{default}}}]}}]}}}}"#
            );
            assert!(
                serde_json::from_str::<RuntimeResponse<InspectPackRuntimeResult>>(&response)
                    .is_ok(),
                "default should be a valid JSON scalar: {default}"
            );
        }
    }

    #[test]
    fn run_runtime_diagnostic_rejects_private_candidate_values() {
        let invocation = RunWorkflowInvocation {
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            dataset: None,
            arguments: Vec::new(),
            candidate_id: "019f6e00-0000-7000-8000-000000000001".to_owned(),
            candidate_path: "C:\\private\\019f6e00-0000-7000-8000-000000000001".to_owned(),
        };
        for private in [
            invocation.candidate_id.clone(),
            invocation.candidate_path.clone(),
            invocation.candidate_path.replace('\\', "/"),
        ] {
            let diagnostic = serde_json::from_value::<KatDiagnostic>(serde_json::json!({
                "message": "Workflow execution failed",
                "causes": [format!("private Runtime value: {private}")],
                "help": "repair the Workflow"
            }))
            .unwrap();
            assert!(exposes_run_private_value(&diagnostic, &invocation));
        }
    }

    #[test]
    fn run_success_rejects_candidate_values_before_the_result_becomes_trusted() {
        let invocation = RunWorkflowInvocation {
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            dataset: None,
            arguments: Vec::new(),
            candidate_id: "019f6e00-0000-7000-8000-000000000001".to_owned(),
            candidate_path: "C:\\private\\019f6e00-0000-7000-8000-000000000001".to_owned(),
        };
        let response = format!(
            r#"{{"status":"success","result":{{"effective_inputs":{{}},"outputs":{{"{}":{{"columns":[{{"name":"value","type":"int64"}}],"row_count":0}}}}}}}}"#,
            invocation.candidate_id
        );

        assert!(
            decode_and_validate_run_workflow_response(response.as_bytes(), &invocation).is_err()
        );
    }

    #[test]
    fn run_success_accepts_named_output_without_a_second_identity() {
        let candidate = tempfile::tempdir().unwrap();
        let output_root = candidate.path().join("outputs");
        fs::create_dir(&output_root).unwrap();
        fs::write(output_root.join("main.parquet"), b"opaque output").unwrap();
        let invocation = RunWorkflowInvocation {
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            dataset: None,
            arguments: Vec::new(),
            candidate_id: "019f6e00-0000-7000-8000-000000000002".to_owned(),
            candidate_path: candidate.path().to_str().unwrap().to_owned(),
        };
        let response = br#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#;

        assert!(decode_and_validate_run_workflow_response(response, &invocation).is_ok());
    }

    #[test]
    fn run_success_does_not_reinspect_runtime_owned_output_files() {
        let candidate = tempfile::tempdir().unwrap();
        let invocation = RunWorkflowInvocation {
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            dataset: None,
            arguments: Vec::new(),
            candidate_id: "019f6e00-0000-7000-8000-000000000002".to_owned(),
            candidate_path: candidate.path().to_str().unwrap().to_owned(),
        };
        let response = br#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#;

        assert!(decode_and_validate_run_workflow_response(response, &invocation).is_ok());
    }

    #[test]
    fn output_names_are_portable_file_names() {
        for reserved in ["con", "prn", "aux", "nul", "com1", "com9", "lpt1", "lpt9"] {
            assert!(!valid_output_name(reserved), "{reserved}");
        }
        for allowed in ["main", "con_data", "com0", "lpt10"] {
            assert!(valid_output_name(allowed), "{allowed}");
        }
    }

    #[test]
    fn run_success_rejects_the_removed_output_id_field() {
        let invocation = RunWorkflowInvocation {
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            dataset: None,
            arguments: Vec::new(),
            candidate_id: "019f6e00-0000-7000-8000-000000000003".to_owned(),
            candidate_path: "C:\\private\\019f6e00-0000-7000-8000-000000000003".to_owned(),
        };
        let response = br#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"output_id":"0123456789abcdef0123456789abcdef","columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#;

        assert!(decode_and_validate_run_workflow_response(response, &invocation).is_err());
    }

    #[test]
    fn run_response_is_strict_and_validates_output_facts() {
        let decode = |value: serde_json::Value| {
            serde_json::from_value::<RuntimeResponse<RawRunWorkflowResult>>(value)
        };
        assert!(
            decode(serde_json::json!({
                "status":"success",
                "result":{"effective_inputs":{},"outputs":{},"extra":true}
            }))
            .is_err()
        );

        let result = |outputs| RawRunWorkflowResult {
            effective_inputs: BTreeMap::new(),
            outputs,
        };
        let candidate = tempfile::tempdir().unwrap();
        let output_root = candidate.path().join("outputs");
        fs::create_dir(&output_root).unwrap();
        fs::write(output_root.join("main.parquet"), b"opaque output").unwrap();
        let invocation = RunWorkflowInvocation {
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            dataset: None,
            arguments: Vec::new(),
            candidate_id: "019f6e00-0000-7000-8000-000000000002".to_owned(),
            candidate_path: candidate.path().to_str().unwrap().to_owned(),
        };
        assert!(validate_run_workflow_report(result(BTreeMap::new()), &invocation).is_err());
        let output = || protocol::RawRuntimeOutput {
            columns: vec![Column {
                name: "value".to_owned(),
                data_type: "int64".to_owned(),
            }],
            row_count: 0,
        };
        assert!(
            validate_run_workflow_report(
                result(BTreeMap::from([("bad-name".to_owned(), output())])),
                &invocation
            )
            .is_err()
        );
        assert!(
            validate_run_workflow_report(
                result(BTreeMap::from([("main".to_owned(), output())])),
                &invocation
            )
            .is_ok()
        );
        assert!(
            validate_run_workflow_report(
                result(BTreeMap::from([(
                    "main".to_owned(),
                    protocol::RawRuntimeOutput {
                        columns: vec![Column {
                            name: String::new(),
                            data_type: "int64".to_owned(),
                        }],
                        row_count: 0,
                    }
                )])),
                &invocation
            )
            .is_err()
        );
        assert!(
            validate_run_workflow_report(
                result(BTreeMap::from([(
                    "main".to_owned(),
                    protocol::RawRuntimeOutput {
                        columns: vec![Column {
                            name: "value".to_owned(),
                            data_type: " ".to_owned(),
                        }],
                        row_count: 0,
                    }
                )])),
                &invocation
            )
            .is_err()
        );
    }

    #[test]
    fn readable_log_faults_are_reported_as_incomplete() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("partial.log");
        fs::write(&path, "partial").unwrap();
        let error = InspectPackInfrastructureError::operation_log(OperationLogError::Write {
            path,
            source: io::Error::other("injected write failure"),
        });

        assert_eq!(
            error.to_string(),
            "PACK inspection Operation log is incomplete"
        );
        assert!(matches!(
            error,
            InspectPackInfrastructureError::IncompleteOperationLog { .. }
        ));
    }

    #[test]
    fn run_log_faults_preserve_the_io_cause_without_exposing_the_candidate() {
        let candidate_id = "019f6e00-0000-7000-8000-000000000004";
        let error = RunWorkflowError::operation_log(OperationLogError::Write {
            path: PathBuf::from(format!(r"C:\data\logs\run-{candidate_id}.log")),
            source: io::Error::other("injected log write failure"),
        });

        let source = std::error::Error::source(&error).unwrap();
        assert_eq!(source.to_string(), "failed to write Operation log");
        assert_eq!(
            source.source().unwrap().to_string(),
            "injected log write failure"
        );
        assert!(!source.to_string().contains(candidate_id));
        assert!(!error.to_string().contains(candidate_id));
    }

    #[test]
    fn runtime_failure_log_keeps_error_on_one_plain_text_line() {
        let error = RuntimeInfrastructureError::InvalidPayload(PathBuf::from(
            "bad\x1b[31m\r\n\tpath\u{0007}",
        ));

        let details = runtime_failure_log_details(&error.to_string());

        assert_eq!(
            details,
            "status: failure\nerror: KAT executable has no Platform Payload directory: bad\\n\\tpath\\u{0007}\n"
        );
        assert!(!details.contains('\x1b'));
        assert!(!details.contains('\r'));
        assert_eq!(details.lines().count(), 2);
    }
}
