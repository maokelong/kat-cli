use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use miette::Diagnostic;
use serde::Serialize;
use thiserror::Error;

use crate::{
    operation_log::{OperationLog, OperationLogError},
    response::KatDiagnostic,
    text_projection::project_inline_text,
};

mod output_spool;
mod protocol;

use output_spool::RuntimeOutputSpool;
use protocol::{InspectPackRuntimeResult, RuntimeResponse};
pub(crate) use protocol::{ParameterDefault, ParameterType, Workflow};

const PRIVATE_RUNTIME_MODULE: &str = "_kat_runtime";

pub(crate) enum InspectPackOutcome {
    Success {
        workflows: Vec<Workflow>,
        log_path: String,
    },
    Failure {
        diagnostic: KatDiagnostic,
        log_path: String,
    },
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
                workflows: result.workflows,
                log_path,
            })
        }
        RuntimeResponse::Failure { error } => {
            if !error.validate() {
                return Err(finish_runtime_error(
                    log,
                    RuntimeInfrastructureError::InvalidResponse(
                        "Runtime Diagnostic contains empty or invalid fields".to_owned(),
                    ),
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

#[derive(Serialize)]
struct InspectPackRequest<'a> {
    operation: &'static str,
    pack_name: &'a str,
    pack_path: &'a str,
}

fn exchange(
    pack_name: &str,
    pack_path: &Path,
    log: &mut OperationLog,
) -> Result<RuntimeResponse<InspectPackRuntimeResult>, ExchangeError> {
    let pack_path_text = pack_path
        .to_str()
        .ok_or_else(|| RuntimeInfrastructureError::NonUnicodePackPath(pack_path.to_path_buf()))?;
    let control = tempfile::Builder::new()
        .prefix("kat-inspect-pack-")
        .tempdir()
        .map_err(RuntimeInfrastructureError::ControlDirectory)?;
    let control_path =
        dunce::canonicalize(control.path()).map_err(RuntimeInfrastructureError::ControlPath)?;
    let request_path = control_path.join("request.json");
    let response_path = control_path.join("response.json");
    let output = RuntimeOutputSpool::create(&control_path)?;
    let (stdout, stderr) = output.stdio()?;
    let request = serde_json::to_vec(&InspectPackRequest {
        operation: "inspect_pack",
        pack_name,
        pack_path: pack_path_text,
    })
    .map_err(RuntimeInfrastructureError::EncodeRequest)?;
    fs::write(&request_path, request).map_err(RuntimeInfrastructureError::WriteRequest)?;

    let python = bundled_python_path()?;
    if !python.is_file() {
        return Err(RuntimeInfrastructureError::MissingHost(python).into());
    }
    let mut child = Command::new(&python)
        .args(["-I", "-B", "-X", "utf8", "-u", "-m", PRIVATE_RUNTIME_MODULE])
        .arg("--request")
        .arg(&request_path)
        .arg("--response")
        .arg(&response_path)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
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
    let response = fs::read(&response_path).map_err(RuntimeInfrastructureError::ReadResponse)?;
    serde_json::from_slice(&response)
        .map_err(RuntimeInfrastructureError::DecodeResponse)
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

fn finish_runtime_error(
    mut log: OperationLog,
    error: RuntimeInfrastructureError,
) -> InspectPackInfrastructureError {
    let details = runtime_failure_log_details(&error);
    if let Err(log_error) = log.append(details.as_bytes()) {
        return InspectPackInfrastructureError::operation_log(log_error);
    }
    match log.finish() {
        Ok(log_path) => InspectPackInfrastructureError::Runtime {
            source: error,
            log_path,
        },
        Err(log_error) => InspectPackInfrastructureError::operation_log(log_error),
    }
}

fn runtime_failure_log_details(error: &RuntimeInfrastructureError) -> String {
    format!(
        "status: failure\nerror: {}\n",
        project_inline_text(&error.to_string())
    )
}

#[derive(Debug)]
enum ExchangeError {
    Log(OperationLogError),
    Runtime(RuntimeInfrastructureError),
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
    #[error("Runtime Response is not valid for inspect_pack")]
    DecodeResponse(#[source] serde_json::Error),
    #[error("Runtime Response is not valid for inspect_pack: {0}")]
    InvalidResponse(String),
}

#[cfg(test)]
mod tests {
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
    fn runtime_failure_log_keeps_error_on_one_plain_text_line() {
        let error = RuntimeInfrastructureError::InvalidPayload(PathBuf::from(
            "bad\x1b[31m\r\n\tpath\u{0007}",
        ));

        let details = runtime_failure_log_details(&error);

        assert_eq!(
            details,
            "status: failure\nerror: KAT executable has no Platform Payload directory: bad\\n\\tpath\\u{0007}\n"
        );
        assert!(!details.contains('\x1b'));
        assert!(!details.contains('\r'));
        assert_eq!(details.lines().count(), 2);
    }
}
