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

mod protocol;
mod stream_capture;

use protocol::{
    InspectPackRuntimeResult, RuntimeFailureOwner, RuntimeResponse, validate_workflows,
};
pub(crate) use protocol::{ParameterDefault, Workflow};
#[cfg(test)]
use stream_capture::RuntimeLogSink;
use stream_capture::capture_streams;

const PRIVATE_RUNTIME_MODULE: &str = "_kat_runtime";

pub(crate) enum InspectPackOutcome {
    Success {
        workflows: Vec<Workflow>,
        log_path: String,
    },
    PackFailure {
        diagnostic: KatDiagnostic,
        log_path: String,
    },
    RuntimeRequestFailure {
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
            if let Err(error) = validate_workflows(&result.workflows) {
                return Err(finish_runtime_error(log, error));
            }
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
        RuntimeResponse::Failure {
            failure_owner,
            error,
        } => {
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
            match failure_owner {
                RuntimeFailureOwner::Pack => Ok(InspectPackOutcome::PackFailure {
                    diagnostic: error,
                    log_path,
                }),
                RuntimeFailureOwner::RuntimeRequest => {
                    Ok(InspectPackOutcome::RuntimeRequestFailure {
                        diagnostic: error,
                        log_path,
                    })
                }
            }
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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| RuntimeInfrastructureError::StartHost { python, source })?;

    let status = match capture_streams(&mut child, log) {
        Ok(status) => status,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
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
    #[error("Bundled Python Host did not expose its {0} pipe")]
    MissingPipe(&'static str),
    #[error("failed to receive captured Runtime output")]
    StreamChannel,
    #[error("failed to read Runtime {stream}")]
    ReadStream {
        stream: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("Runtime {0} capture thread failed")]
    StreamThread(&'static str),
    #[error("Runtime output made no progress while draining after Bundled Python Host exit")]
    StreamDrainTimeout,
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
    use crate::text_projection::TextProjection;
    use std::io::Write as _;
    use std::thread;
    use std::time::{Duration, Instant};

    struct FailingLog {
        path: PathBuf,
    }

    #[derive(Default)]
    struct RecordingLog {
        bytes: Vec<u8>,
    }

    struct SlowRecordingLog {
        bytes: Vec<u8>,
        delay: Duration,
    }

    impl RuntimeLogSink for RecordingLog {
        fn append(&mut self, bytes: &[u8]) -> Result<(), OperationLogError> {
            self.bytes.extend_from_slice(bytes);
            Ok(())
        }
    }

    impl RuntimeLogSink for SlowRecordingLog {
        fn append(&mut self, bytes: &[u8]) -> Result<(), OperationLogError> {
            if !bytes.is_empty() {
                thread::sleep(self.delay);
                self.bytes.extend_from_slice(bytes);
            }
            Ok(())
        }
    }

    impl RuntimeLogSink for FailingLog {
        fn append(&mut self, _bytes: &[u8]) -> Result<(), OperationLogError> {
            Err(OperationLogError::Write {
                path: self.path.clone(),
                source: io::Error::other("injected log write failure"),
            })
        }
    }

    #[test]
    fn text_projection_is_streaming_plain_utf8_with_stable_newlines() {
        let mut projection = TextProjection::new("stdout");
        let mut output = projection.push(b"first\r");
        output.push_str(&projection.push(b"\n\xF0\x9F"));
        output.push_str(&projection.push(b"\x98\x80\x00\rthird\xFF"));
        output.push_str(&projection.finish());

        assert_eq!(
            output,
            "first\n😀\\u{0000}\nthird[KAT: invalid UTF-8 in Runtime stdout was replaced]\n�"
        );

        let mut boundary = TextProjection::new("stderr");
        let mut output = boundary.push(b"line\r\xFF");
        output.push_str(&boundary.finish());
        assert_eq!(
            output,
            "line\n[KAT: invalid UTF-8 in Runtime stderr was replaced]\n�"
        );
    }

    #[test]
    fn strict_response_rejects_unknown_fields_and_incomplete_parameter_contracts() {
        let unknown = br#"{"status":"success","result":{"workflows":[],"extra":true}}"#;
        assert!(
            serde_json::from_slice::<RuntimeResponse<InspectPackRuntimeResult>>(unknown).is_err()
        );

        let missing_failure_owner = br#"{"status":"failure","error":{"message":"failed"}}"#;
        assert!(
            serde_json::from_slice::<RuntimeResponse<InspectPackRuntimeResult>>(
                missing_failure_owner
            )
            .is_err()
        );
        let unknown_failure_owner =
            br#"{"status":"failure","failure_owner":"deployment","error":{"message":"failed"}}"#;
        assert!(
            serde_json::from_slice::<RuntimeResponse<InspectPackRuntimeResult>>(
                unknown_failure_owner
            )
            .is_err()
        );
        let pack_failure =
            br#"{"status":"failure","failure_owner":"pack","error":{"message":"failed"}}"#;
        assert!(matches!(
            serde_json::from_slice::<RuntimeResponse<InspectPackRuntimeResult>>(pack_failure)
                .unwrap(),
            RuntimeResponse::Failure {
                failure_owner: RuntimeFailureOwner::Pack,
                ..
            }
        ));
        let request_failure = br#"{"status":"failure","failure_owner":"runtime_request","error":{"message":"failed"}}"#;
        assert!(matches!(
            serde_json::from_slice::<RuntimeResponse<InspectPackRuntimeResult>>(request_failure)
                .unwrap(),
            RuntimeResponse::Failure {
                failure_owner: RuntimeFailureOwner::RuntimeRequest,
                ..
            }
        ));

        let response = br#"{"status":"success","result":{"workflows":[{"name":"a","title":"A","description":"A","required_tables":[],"parameters":[{"name":"flag","option":"--flag","type":"boolean","required":false,"description":"Flag","default":false}]}]}}"#;
        let RuntimeResponse::Success { result } =
            serde_json::from_slice::<RuntimeResponse<InspectPackRuntimeResult>>(response).unwrap()
        else {
            panic!("expected success response");
        };
        assert!(validate_workflows(&result.workflows).is_err());

        let validate = |workflow: serde_json::Value| {
            let response = serde_json::json!({
                "status": "success",
                "result": {"workflows": [workflow]}
            });
            let RuntimeResponse::Success { result } =
                serde_json::from_value::<RuntimeResponse<InspectPackRuntimeResult>>(response)
                    .unwrap()
            else {
                panic!("expected success response");
            };
            validate_workflows(&result.workflows).is_ok()
        };
        let workflow = |name: &str, tables: serde_json::Value, parameter: serde_json::Value| {
            serde_json::json!({
                "name": name,
                "title": "A",
                "description": "A",
                "required_tables": tables,
                "parameters": [parameter]
            })
        };
        let parameter = |name: &str, parameter_type: &str, default: serde_json::Value| {
            serde_json::json!({
                "name": name,
                "option": format!("--{}", name.replace('_', "-")),
                "type": parameter_type,
                "required": false,
                "description": "Value",
                "default": default
            })
        };

        assert!(validate(workflow(
            "valid-name",
            serde_json::json!(["thread"]),
            parameter("window", "duration", serde_json::json!("0.125ms"))
        )));
        assert!(validate(workflow(
            "valid-wall-clock",
            serde_json::json!([]),
            parameter(
                "at",
                "wall_clock_timestamp",
                serde_json::json!("2026-07-17T12:00:00.123456789Z"),
            )
        )));
        for invalid in [
            workflow(
                "invalid_name",
                serde_json::json!([]),
                parameter("value", "string", serde_json::json!(null)),
            ),
            workflow(
                "invalid-table",
                serde_json::json!(["con"]),
                parameter("value", "string", serde_json::json!(null)),
            ),
            workflow(
                "invalid-boolean",
                serde_json::json!([]),
                serde_json::json!({
                    "name": "flag",
                    "option": "--flag",
                    "negative_option": "--no-flag",
                    "type": "boolean",
                    "required": false,
                    "description": "Flag",
                    "default": null
                }),
            ),
            workflow(
                "invalid-duration",
                serde_json::json!([]),
                parameter("window", "duration", serde_json::json!("0.1ns")),
            ),
            workflow(
                "invalid-wall-clock",
                serde_json::json!([]),
                parameter(
                    "at",
                    "wall_clock_timestamp",
                    serde_json::json!("2026-07-17T12:00:00+00:00"),
                ),
            ),
            workflow(
                "invalid-option",
                serde_json::json!([]),
                serde_json::json!({
                    "name": "value",
                    "option": "--other",
                    "type": "string",
                    "required": false,
                    "description": "Value",
                    "default": ""
                }),
            ),
        ] {
            assert!(!validate(invalid));
        }
    }

    #[test]
    fn readable_log_faults_are_reported_as_incomplete() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("partial.log");
        fs::write(&path, "partial\n").unwrap();

        let error = InspectPackInfrastructureError::operation_log(OperationLogError::Flush {
            path,
            source: io::Error::other("injected flush failure"),
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
    #[allow(clippy::zombie_processes)] // exit cases intentionally leave pipe-owning descendants to the parent test
    fn inherited_pipe_helper() {
        match std::env::var("KAT_INHERITED_PIPE_HELPER").as_deref() {
            Ok("direct") => {
                let executable = std::env::current_exe().unwrap();
                let mut descendant = Command::new(executable)
                    .args([
                        "--exact",
                        "workflow_runtime::tests::inherited_pipe_helper",
                        "--nocapture",
                    ])
                    .env("KAT_INHERITED_PIPE_HELPER", "descendant")
                    .stdin(Stdio::null())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap();
                println!("direct Runtime output");
                io::stdout().flush().unwrap();
                thread::sleep(Duration::from_secs(30));
                descendant.wait().unwrap();
            }
            Ok("direct-exit") => {
                let executable = std::env::current_exe().unwrap();
                Command::new(executable)
                    .args([
                        "--exact",
                        "workflow_runtime::tests::inherited_pipe_helper",
                        "--nocapture",
                    ])
                    .env("KAT_INHERITED_PIPE_HELPER", "descendant")
                    .stdin(Stdio::null())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap();
                println!("direct Runtime output");
                io::stdout().flush().unwrap();
            }
            Ok("burst-exit") => {
                let mut stdout = io::stdout();
                stdout.write_all(&vec![b'x'; 512 * 1024]).unwrap();
                stdout.flush().unwrap();
            }
            Ok(mode @ ("late-exit" | "active-exit")) => {
                let executable = std::env::current_exe().unwrap();
                let descendant_mode = if mode == "late-exit" {
                    "late-descendant"
                } else {
                    "active-descendant"
                };
                Command::new(executable)
                    .args([
                        "--exact",
                        "workflow_runtime::tests::inherited_pipe_helper",
                        "--nocapture",
                    ])
                    .env("KAT_INHERITED_PIPE_HELPER", descendant_mode)
                    .stdin(Stdio::null())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap();
            }
            Ok("late-descendant") => {
                thread::sleep(Duration::from_millis(250));
                println!("\x1b[31mlate Runtime output \u{1f600}\x1b[0m");
                io::stdout().flush().unwrap();
            }
            Ok("active-descendant") => {
                for index in 0..4 {
                    thread::sleep(Duration::from_millis(700));
                    println!("active Runtime output {index}");
                    io::stdout().flush().unwrap();
                }
            }
            Ok("descendant") => thread::sleep(Duration::from_secs(3)),
            _ => {}
        }
    }

    #[test]
    fn runtime_exit_reports_stalled_inherited_pipes_as_failure() {
        let executable = std::env::current_exe().unwrap();
        let mut child = Command::new(executable)
            .args([
                "--exact",
                "workflow_runtime::tests::inherited_pipe_helper",
                "--nocapture",
            ])
            .env("KAT_INHERITED_PIPE_HELPER", "direct-exit")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut log = RecordingLog::default();
        let started = Instant::now();

        let result = capture_streams(&mut child, &mut log);

        assert!(matches!(
            result,
            Err(ExchangeError::Runtime(
                RuntimeInfrastructureError::StreamDrainTimeout
            ))
        ));
        assert!(started.elapsed() >= Duration::from_secs(2));
        assert!(
            String::from_utf8(log.bytes)
                .unwrap()
                .contains("direct Runtime output\n")
        );
        thread::sleep(Duration::from_secs(2));
    }

    #[test]
    fn runtime_exit_captures_late_output_before_eof() {
        let executable = std::env::current_exe().unwrap();
        let mut child = Command::new(executable)
            .args([
                "--exact",
                "workflow_runtime::tests::inherited_pipe_helper",
                "--nocapture",
            ])
            .env("KAT_INHERITED_PIPE_HELPER", "late-exit")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut log = RecordingLog::default();
        let started = Instant::now();

        let Ok(status) = capture_streams(&mut child, &mut log) else {
            panic!("capture must succeed");
        };

        assert!(status.success());
        assert!(started.elapsed() >= Duration::from_millis(200));
        let log = String::from_utf8(log.bytes).unwrap();
        assert!(log.contains("late Runtime output \u{1f600}\n"));
        assert!(!log.contains('\x1b'));
    }

    #[test]
    fn runtime_exit_allows_active_drain_beyond_stall_limit() {
        let executable = std::env::current_exe().unwrap();
        let mut child = Command::new(executable)
            .args([
                "--exact",
                "workflow_runtime::tests::inherited_pipe_helper",
                "--nocapture",
            ])
            .env("KAT_INHERITED_PIPE_HELPER", "active-exit")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut log = RecordingLog::default();
        let started = Instant::now();

        let Ok(status) = capture_streams(&mut child, &mut log) else {
            panic!("capture must succeed");
        };

        assert!(status.success());
        assert!(started.elapsed() >= Duration::from_secs(2));
        assert!(
            String::from_utf8(log.bytes)
                .unwrap()
                .contains("active Runtime output 3\n")
        );
    }

    #[test]
    fn runtime_exit_drains_queued_output_before_reporting_success() {
        let executable = std::env::current_exe().unwrap();
        let mut child = Command::new(executable)
            .args([
                "--exact",
                "workflow_runtime::tests::inherited_pipe_helper",
                "--nocapture",
            ])
            .env("KAT_INHERITED_PIPE_HELPER", "burst-exit")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut log = SlowRecordingLog {
            bytes: Vec::new(),
            delay: Duration::from_millis(5),
        };

        let Ok(status) = capture_streams(&mut child, &mut log) else {
            panic!("capture must succeed");
        };

        assert!(status.success());
        assert!(log.bytes.iter().filter(|byte| **byte == b'x').count() >= 512 * 1024);
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

    #[test]
    fn log_failure_reaps_runtime_without_waiting_for_inherited_pipes() {
        let temporary = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let mut child = Command::new(executable)
            .args([
                "--exact",
                "workflow_runtime::tests::inherited_pipe_helper",
                "--nocapture",
            ])
            .env("KAT_INHERITED_PIPE_HELPER", "direct")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut log = FailingLog {
            path: temporary.path().join("partial.log"),
        };
        let started = Instant::now();

        let result = capture_streams(&mut child, &mut log);

        assert!(matches!(result, Err(ExchangeError::Log(_))));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(child.try_wait().unwrap().is_some());
        thread::sleep(Duration::from_secs(4));
    }
}
