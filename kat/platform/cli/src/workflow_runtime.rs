use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
};

use miette::Diagnostic;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    operation_log::{OperationLog, OperationLogError},
    response::KatDiagnostic,
    text_projection::project_inline_text,
};

mod output_spool;
mod protocol;

use output_spool::{RuntimeOutputMirror, RuntimeOutputSpool};
pub(crate) use protocol::{
    Column, NestedRelation, NestedRunCall, NestedRunOutcome, TestControlCall, TestControlOutcome,
    WorkflowInputs,
};
use protocol::{
    InspectProviderRequest, InspectProviderResult, InspectProvidersResult, InspectWorkflowRequest,
    InspectWorkflowResult, InspectWorkflowsResult, NestedRunRequestFrame, NestedRunResponseFrame,
    QueryRunRequest, RawRunWorkflowResult, RunWorkflowRequest, RuntimeResponse,
    TestControlRequestFrame, TestPackRequest, TestPackResult,
};

const PRIVATE_RUNTIME_MODULE: &str = "_kat_runtime";

/// Executes one nested Workflow request received from a parent Runtime.
///
/// The callback owns all Session, discovery, recursion, and publication policy.
/// Its failure message must be safe to expose to PACK code as `kat.RunError`.
pub(crate) trait NestedRunCallback: Send + Sync {
    fn execute(&self, call: NestedRunCall) -> NestedRunOutcome;
    fn take_logs(&self) -> Vec<String> {
        Vec::new()
    }
}

impl<F> NestedRunCallback for F
where
    F: Fn(NestedRunCall) -> NestedRunOutcome + Send + Sync,
{
    fn execute(&self, call: NestedRunCall) -> NestedRunOutcome {
        self(call)
    }
}

pub(crate) trait TestControlCallback: Send + Sync {
    fn execute(&self, call: TestControlCall) -> TestControlOutcome;
    fn take_logs(&self) -> Vec<String> {
        Vec::new()
    }
}

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

impl<T> RuntimeOutcome<T> {
    pub(crate) fn map<U>(self, project: impl FnOnce(T) -> U) -> RuntimeOutcome<U> {
        match self {
            Self::Success { result, log_path } => RuntimeOutcome::Success {
                result: project(result),
                log_path,
            },
            Self::Failure {
                diagnostic,
                log_path,
            } => RuntimeOutcome::Failure {
                diagnostic,
                log_path,
            },
        }
    }
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum WorkflowInspectionResult {
    List(InspectWorkflowsResult),
    Detail(InspectWorkflowResult),
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum ProviderInspectionResult {
    List(InspectProvidersResult),
    Detail(InspectProviderResult),
}

pub(crate) enum QueryRunOutcome {
    Success {
        result: QueryRunResult,
        log: OperationLog,
    },
    Failure {
        diagnostic: KatDiagnostic,
        log: OperationLog,
    },
}

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
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunOutputMetadata {
    pub(crate) columns: Vec<Column>,
    pub(crate) row_count: u64,
}

pub(crate) struct RunWorkflowInvocation {
    pub(crate) session_id: String,
    pub(crate) pack_name: String,
    pub(crate) pack_path: String,
    pub(crate) workflow_name: String,
    pub(crate) input: WorkflowInputs,
    pub(crate) candidate_id: String,
    pub(crate) candidate_path: String,
    pub(crate) datasource_root: String,
    pub(crate) scratch_root: String,
}

fn run_workflow_runtime_request(invocation: &RunWorkflowInvocation) -> RunWorkflowRequest<'_> {
    RunWorkflowRequest {
        operation: "run_workflow",
        pack_name: &invocation.pack_name,
        pack_path: &invocation.pack_path,
        workflow_name: &invocation.workflow_name,
        input: &invocation.input,
        candidate_id: &invocation.candidate_id,
        candidate_path: &invocation.candidate_path,
        datasource_root: &invocation.datasource_root,
        scratch_root: &invocation.scratch_root,
    }
}

pub(crate) struct QueryRunInvocation {
    pub(crate) outputs: BTreeMap<String, String>,
    pub(crate) sql: String,
    pub(crate) result_path: String,
}

pub(crate) struct TestPackInvocation<'a> {
    pub(crate) pack_name: &'a str,
    pub(crate) pack_path: &'a Path,
    pub(crate) tests: &'a [String],
    pub(crate) test_report_path: &'a Path,
    pub(crate) callback: Arc<dyn TestControlCallback>,
}

pub(crate) type TestPackError = RunWorkflowError;

pub(crate) enum TestPackOutcome {
    Success {
        result: TestPackResult,
        log_path: String,
    },
    Failure {
        diagnostic: KatDiagnostic,
        log_path: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QueryRunResult {
    pub(crate) columns: Vec<Column>,
}

pub(crate) fn inspect_workflow(
    log: OperationLog,
    pack_name: &str,
    pack_path: &Path,
    workflow_name: Option<&str>,
) -> Result<RuntimeOutcome<WorkflowInspectionResult>, InspectPackInfrastructureError> {
    let Some(pack_path_text) = pack_path.to_str() else {
        return Err(finish_runtime_error(
            log,
            RuntimeInfrastructureError::NonUnicodePackPath(pack_path.to_path_buf()),
        ));
    };
    let request = InspectWorkflowRequest {
        operation: "inspect_workflow",
        pack_name,
        pack_path: pack_path_text,
        workflow_name,
    };
    if workflow_name.is_some() {
        let outcome: RuntimeOutcome<InspectWorkflowResult> =
            inspect_request(log, "kat-inspect-workflow-", &request)?;
        Ok(outcome.map(WorkflowInspectionResult::Detail))
    } else {
        let outcome: RuntimeOutcome<InspectWorkflowsResult> =
            inspect_request(log, "kat-inspect-workflow-", &request)?;
        Ok(outcome.map(WorkflowInspectionResult::List))
    }
}

pub(crate) fn inspect_provider(
    log: OperationLog,
    pack_name: &str,
    pack_path: &Path,
    provider_name: Option<&str>,
) -> Result<RuntimeOutcome<ProviderInspectionResult>, InspectPackInfrastructureError> {
    let Some(pack_path_text) = pack_path.to_str() else {
        return Err(finish_runtime_error(
            log,
            RuntimeInfrastructureError::NonUnicodePackPath(pack_path.to_path_buf()),
        ));
    };
    let request = InspectProviderRequest {
        operation: "inspect_provider",
        pack_name,
        pack_path: pack_path_text,
        provider_name,
    };
    if provider_name.is_some() {
        let outcome: RuntimeOutcome<InspectProviderResult> =
            inspect_request(log, "kat-inspect-provider-", &request)?;
        Ok(outcome.map(ProviderInspectionResult::Detail))
    } else {
        let outcome: RuntimeOutcome<InspectProvidersResult> =
            inspect_request(log, "kat-inspect-provider-", &request)?;
        Ok(outcome.map(ProviderInspectionResult::List))
    }
}

fn inspect_request<R: DeserializeOwned>(
    mut log: OperationLog,
    prefix: &str,
    request: &impl Serialize,
) -> Result<RuntimeOutcome<R>, InspectPackInfrastructureError> {
    let response = match exchange_request(prefix, request, &mut log) {
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
            Ok(RuntimeOutcome::Success { result, log_path })
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
            Ok(RuntimeOutcome::Failure {
                diagnostic: error,
                log_path,
            })
        }
    }
}

pub(crate) fn execute_workflow_runtime(
    mut log: OperationLog,
    invocation: RunWorkflowInvocation,
    callback: Arc<dyn NestedRunCallback>,
) -> Result<RunWorkflowOutcome, RunWorkflowError> {
    let request = run_workflow_runtime_request(&invocation);
    let exchanged =
        exchange_run_workflow_bytes("kat-run-workflow-", &request, &mut log, callback.clone());
    for note in callback.take_logs() {
        log.append(format!("{note}\n").as_bytes())
            .map_err(RunWorkflowError::operation_log)?;
    }
    let response = match exchanged {
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
            if let Err(source) = log.append(
                format!(
                    "runtime_status: failure\ndiagnostic: {}\n",
                    project_inline_text(&error.reason())
                )
                .as_bytes(),
            ) {
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

pub(crate) fn execute_query_runtime(
    mut log: OperationLog,
    invocation: QueryRunInvocation,
) -> Result<QueryRunOutcome, QueryRunError> {
    let request = QueryRunRequest {
        operation: "query_run",
        outputs: &invocation.outputs,
        sql: &invocation.sql,
        result_path: &invocation.result_path,
    };
    let response = match exchange_request("kat-query-run-", &request, &mut log) {
        Ok(response) => response,
        Err(ExchangeError::Log(error)) => return Err(QueryRunError::operation_log(error)),
        Err(ExchangeError::Runtime(error)) => return Err(finish_runtime_error(log, error)),
        Err(ExchangeError::InvalidResponse(details)) => {
            return Err(finish_invalid_runtime_response(log, &details));
        }
    };
    match response {
        RuntimeResponse::Success { result } => {
            if let Err(source) = log.append(b"runtime_status: success\n") {
                return Err(QueryRunError::operation_log(source));
            }
            Ok(QueryRunOutcome::Success { result, log })
        }
        RuntimeResponse::Failure { error } => {
            if let Err(source) = log.append(b"runtime_status: failure\n") {
                return Err(QueryRunError::operation_log(source));
            }
            Ok(QueryRunOutcome::Failure {
                diagnostic: error,
                log,
            })
        }
    }
}

pub(crate) fn test_pack(
    mut log: OperationLog,
    invocation: TestPackInvocation<'_>,
) -> Result<TestPackOutcome, TestPackError> {
    let pack_path = match invocation.pack_path.to_str() {
        Some(path) => path,
        None => {
            return Err(finish_runtime_error(
                log,
                RuntimeInfrastructureError::NonUnicodePackPath(invocation.pack_path.to_path_buf()),
            ));
        }
    };
    let request = TestPackRequest {
        operation: "test_pack",
        pack_name: invocation.pack_name,
        pack_path,
        tests: invocation.tests,
    };
    let exchanged = exchange_test_request(
        "kat-test-pack-",
        &request,
        &mut log,
        invocation.pack_path,
        invocation.test_report_path,
        invocation.callback.clone(),
    );
    for note in invocation.callback.take_logs() {
        log.append(format!("{note}\n").as_bytes())
            .map_err(RunWorkflowError::operation_log)?;
    }
    let response: RuntimeResponse<TestPackResult> = match exchanged {
        Ok(response) => response,
        Err(ExchangeError::Log(error)) => return Err(RunWorkflowError::operation_log(error)),
        Err(ExchangeError::Runtime(error)) => return Err(finish_runtime_error(log, error)),
        Err(ExchangeError::InvalidResponse(details)) => {
            return Err(finish_invalid_runtime_response(log, &details));
        }
    };
    match response {
        RuntimeResponse::Success { result } => {
            log.append(b"runtime_status: success\n")
                .map_err(RunWorkflowError::operation_log)?;
            Ok(TestPackOutcome::Success {
                result,
                log_path: log.finish().map_err(RunWorkflowError::operation_log)?,
            })
        }
        RuntimeResponse::Failure { error } => {
            log.append(b"runtime_status: failure\n")
                .map_err(RunWorkflowError::operation_log)?;
            Ok(TestPackOutcome::Failure {
                diagnostic: error,
                log_path: log.finish().map_err(RunWorkflowError::operation_log)?,
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
        invocation.session_id.clone(),
        invocation.candidate_id.clone(),
        invocation.candidate_path.clone(),
        invocation.candidate_path.replace('\\', "/"),
        invocation.datasource_root.clone(),
        invocation.datasource_root.replace('\\', "/"),
        invocation.scratch_root.clone(),
        invocation.scratch_root.replace('\\', "/"),
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
    let contains_private = |value: &str| {
        value.contains(&invocation.candidate_id)
            || contains_private_path(value, &invocation.candidate_path)
            || contains_private_path(value, &invocation.datasource_root)
            || contains_private_path(value, &invocation.scratch_root)
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

fn contains_private_path(value: &str, private_path: &str) -> bool {
    value.contains(private_path) || value.contains(&private_path.replace('\\', "/"))
}

pub(crate) fn valid_output_name(name: &str) -> bool {
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

fn exchange_run_workflow_bytes(
    prefix: &str,
    request: &impl Serialize,
    log: &mut OperationLog,
    callback: Arc<dyn NestedRunCallback>,
) -> Result<Vec<u8>, ExchangeError> {
    let control = prepare_runtime_control(prefix, request)?;
    let output = RuntimeOutputSpool::create_stderr_only(&control.path)?;
    let stderr = output.stderr_stdio()?;
    let (mut command, python) = runtime_command(&control)?;
    command
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr);
    let mut child = command
        .spawn()
        .map_err(|source| RuntimeInfrastructureError::StartHost { python, source })?;
    drop(command);
    let stdin = child
        .stdin
        .take()
        .ok_or(RuntimeInfrastructureError::MissingHostStdin)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(RuntimeInfrastructureError::MissingHostStdout)?;
    let pump_result = pump_nested_requests(
        BufReader::new(stdout),
        Arc::new(Mutex::new(stdin)),
        callback,
    );
    let status = if pump_result.is_ok() {
        match child.wait() {
            Ok(status) => Some(status),
            Err(source) => {
                terminate_test_runtime(&mut child);
                output.append_stderr_to(log)?;
                return Err(RuntimeInfrastructureError::WaitHost(source).into());
            }
        }
    } else {
        terminate_test_runtime(&mut child);
        None
    };
    output.append_stderr_to(log)?;
    pump_result?;
    let status = status.expect("a successful control pump waits for its Runtime");
    if !status.success() {
        return Err(RuntimeInfrastructureError::HostExit(status.code()).into());
    }
    read_runtime_response(&control).map_err(Into::into)
}

fn pump_nested_requests<R, W, C>(
    reader: R,
    writer: Arc<Mutex<W>>,
    callback: Arc<C>,
) -> Result<(), RuntimeInfrastructureError>
where
    R: BufRead + Send + 'static,
    W: Write + Send + 'static,
    C: NestedRunCallback + ?Sized + 'static,
{
    pump_control_requests(
        reader,
        writer,
        |bytes| {
            let frame: NestedRunRequestFrame = serde_json::from_slice(bytes)
                .map_err(|_| RuntimeInfrastructureError::InvalidNestedRequest)?;
            let id = frame.call_id;
            let call = frame.into_call();
            if !valid_nested_call(&call) {
                return Err(RuntimeInfrastructureError::InvalidNestedRequest);
            }
            Ok((id, call))
        },
        move |id, call| {
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback.execute(call)))
                    .unwrap_or_else(|_| NestedRunOutcome::Failure {
                        message: "nested Workflow Host callback failed".to_owned(),
                    });
            serde_json::to_value(NestedRunResponseFrame::from_outcome(
                id,
                validate_nested_outcome(outcome),
            ))
            .expect("nested response contains only JSON values")
        },
    )
}

enum ControlPumpEvent<T> {
    Request((u64, T)),
    ReaderFinished(Result<(), RuntimeInfrastructureError>),
    WorkerFinished(Result<(), RuntimeInfrastructureError>),
}

fn pump_control_requests<R, W, T, D, F>(
    reader: R,
    writer: Arc<Mutex<W>>,
    decode: D,
    respond: F,
) -> Result<(), RuntimeInfrastructureError>
where
    R: BufRead + Send + 'static,
    W: Write + Send + 'static,
    T: Send + 'static,
    D: Fn(&[u8]) -> Result<(u64, T), RuntimeInfrastructureError> + Send + 'static,
    F: Fn(u64, T) -> serde_json::Value + Send + Sync + 'static,
{
    let respond = Arc::new(respond);
    let (events, received_events) = mpsc::channel();
    let reader_events = events.clone();
    let reader_worker = thread::Builder::new()
        .name("kat-nested-run-reader".to_owned())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                read_control_requests(reader, &reader_events, decode)
            }))
            .unwrap_or(Err(RuntimeInfrastructureError::NestedRequestReaderPanicked));
            let _ = reader_events.send(ControlPumpEvent::ReaderFinished(result));
        })
        .map_err(RuntimeInfrastructureError::StartNestedRequestReader)?;

    let mut seen_call_ids = BTreeSet::new();
    let mut workers = Vec::new();
    let mut pump_error = None;
    let mut reader_finished = false;
    let mut active_workers = 0usize;
    loop {
        if reader_finished && active_workers == 0 {
            break;
        }
        let event = match received_events.recv() {
            Ok(event) => event,
            Err(_) => {
                pump_error = Some(RuntimeInfrastructureError::NestedControlPumpStopped);
                break;
            }
        };
        match event {
            ControlPumpEvent::Request(frame) => {
                let (call_id, call) = frame;
                if !seen_call_ids.insert(call_id) {
                    pump_error = Some(RuntimeInfrastructureError::InvalidNestedRequest);
                    break;
                }
                let writer = Arc::clone(&writer);
                let respond = Arc::clone(&respond);
                let worker_events = events.clone();
                let worker = match thread::Builder::new()
                    .name(format!("kat-nested-run-{call_id}"))
                    .spawn(move || {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            write_control_response(writer, respond(call_id, call))
                        }))
                        .unwrap_or(Err(RuntimeInfrastructureError::NestedWorkerPanicked));
                        let _ = worker_events.send(ControlPumpEvent::WorkerFinished(result));
                    }) {
                    Ok(worker) => worker,
                    Err(source) => {
                        pump_error = Some(RuntimeInfrastructureError::StartNestedWorker(source));
                        break;
                    }
                };
                workers.push(worker);
                active_workers += 1;
            }
            ControlPumpEvent::ReaderFinished(result) => {
                reader_finished = true;
                if let Err(error) = result {
                    pump_error = Some(error);
                    break;
                }
            }
            ControlPumpEvent::WorkerFinished(result) => {
                active_workers = active_workers
                    .checked_sub(1)
                    .expect("a nested worker completion follows its request");
                if let Err(error) = result {
                    pump_error = Some(error);
                    break;
                }
            }
        }
    }

    for worker in workers {
        if worker.join().is_err() && pump_error.is_none() {
            pump_error = Some(RuntimeInfrastructureError::NestedWorkerPanicked);
        }
    }
    drop(writer);
    if reader_finished && reader_worker.join().is_err() && pump_error.is_none() {
        pump_error = Some(RuntimeInfrastructureError::NestedRequestReaderPanicked);
    }
    match pump_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn read_control_requests<R, T, D>(
    mut reader: R,
    events: &mpsc::Sender<ControlPumpEvent<T>>,
    decode: D,
) -> Result<(), RuntimeInfrastructureError>
where
    R: BufRead,
    D: Fn(&[u8]) -> Result<(u64, T), RuntimeInfrastructureError>,
{
    loop {
        let mut bytes = Vec::new();
        let count = reader
            .read_until(b'\n', &mut bytes)
            .map_err(RuntimeInfrastructureError::ReadNestedRequest)?;
        if count == 0 {
            return Ok(());
        }
        if bytes.last() != Some(&b'\n') {
            return Err(RuntimeInfrastructureError::InvalidNestedRequest);
        }
        if events
            .send(ControlPumpEvent::Request(decode(&bytes)?))
            .is_err()
        {
            return Ok(());
        }
    }
}

fn valid_nested_call(call: &NestedRunCall) -> bool {
    !call.pack_name.is_empty()
        && !call.workflow_name.is_empty()
        && call.inputs.keys().all(|name| !name.is_empty())
}

fn write_control_response<W: Write>(
    writer: Arc<Mutex<W>>,
    response: serde_json::Value,
) -> Result<(), RuntimeInfrastructureError> {
    let mut frame =
        serde_json::to_vec(&response).map_err(RuntimeInfrastructureError::EncodeNestedResponse)?;
    frame.push(b'\n');
    let mut writer = writer
        .lock()
        .map_err(|_| RuntimeInfrastructureError::NestedResponseWriterPoisoned)?;
    writer
        .write_all(&frame)
        .and_then(|()| writer.flush())
        .map_err(RuntimeInfrastructureError::WriteNestedResponse)
}

fn validate_nested_outcome(outcome: NestedRunOutcome) -> NestedRunOutcome {
    let valid = match &outcome {
        NestedRunOutcome::Success { relations } => {
            relations
                .iter()
                .all(|relation| valid_output_name(&relation.name) && !relation.path.is_empty())
                && relations.windows(2).all(|pair| pair[0].name < pair[1].name)
        }
        NestedRunOutcome::Failure { message } => !message.trim().is_empty(),
    };
    if valid {
        outcome
    } else {
        NestedRunOutcome::Failure {
            message: "nested Workflow Host returned an invalid result".to_owned(),
        }
    }
}

fn pump_test_requests<R, W, C>(
    reader: R,
    writer: Arc<Mutex<W>>,
    callback: Arc<C>,
) -> Result<(), RuntimeInfrastructureError>
where
    R: BufRead + Send + 'static,
    W: Write + Send + 'static,
    C: TestControlCallback + ?Sized + 'static,
{
    pump_control_requests(
        reader,
        writer,
        |bytes| {
            let frame: TestControlRequestFrame = serde_json::from_slice(bytes)
                .map_err(|_| RuntimeInfrastructureError::InvalidNestedRequest)?;
            let (id, call) = frame.into_call();
            let valid = match &call {
                TestControlCall::BeginSession => true,
                TestControlCall::RunWorkflow {
                    test_session_id,
                    pack_name,
                    workflow_name,
                    ..
                } => {
                    !test_session_id.is_empty()
                        && !pack_name.is_empty()
                        && !workflow_name.is_empty()
                }
                TestControlCall::EndSession { test_session_id } => !test_session_id.is_empty(),
            };
            if !valid {
                return Err(RuntimeInfrastructureError::InvalidNestedRequest);
            }
            Ok((id, call))
        },
        move |id, call| {
            let kind = match &call {
                TestControlCall::BeginSession => TestCallKind::BeginSession,
                TestControlCall::RunWorkflow { .. } => TestCallKind::RunWorkflow,
                TestControlCall::EndSession { .. } => TestCallKind::EndSession,
            };
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback.execute(call)))
                    .unwrap_or_else(|_| TestControlOutcome::Failure {
                        message: "PACK test Host callback failed".to_owned(),
                    });
            test_response_value(id, kind, outcome)
        },
    )
}

#[derive(Clone, Copy)]
enum TestCallKind {
    BeginSession,
    RunWorkflow,
    EndSession,
}

fn test_response_value(
    call_id: u64,
    kind: TestCallKind,
    outcome: TestControlOutcome,
) -> serde_json::Value {
    match (kind, outcome) {
        (TestCallKind::BeginSession, TestControlOutcome::SessionStarted { test_session_id })
            if !test_session_id.is_empty() =>
        {
            serde_json::json!({
                "call_id": call_id,
                "status": "success",
                "test_session_id": test_session_id,
            })
        }
        (
            TestCallKind::RunWorkflow,
            TestControlOutcome::Workflow(NestedRunOutcome::Success { relations }),
        ) => match validate_nested_outcome(NestedRunOutcome::Success { relations }) {
            NestedRunOutcome::Success { relations } => serde_json::json!({
                "call_id": call_id,
                "status": "success",
                "relations": relations,
            }),
            NestedRunOutcome::Failure { message } => test_failure_value(call_id, message),
        },
        (
            TestCallKind::RunWorkflow,
            TestControlOutcome::Workflow(NestedRunOutcome::Failure { message }),
        ) => match validate_nested_outcome(NestedRunOutcome::Failure { message }) {
            NestedRunOutcome::Failure { message } => test_failure_value(call_id, message),
            NestedRunOutcome::Success { .. } => unreachable!(),
        },
        (TestCallKind::EndSession, TestControlOutcome::Complete) => {
            serde_json::json!({"call_id": call_id, "status": "success"})
        }
        (_, TestControlOutcome::Failure { message }) if !message.trim().is_empty() => {
            test_failure_value(call_id, message)
        }
        _ => test_failure_value(
            call_id,
            "PACK test Host returned an invalid result".to_owned(),
        ),
    }
}

fn test_failure_value(call_id: u64, message: String) -> serde_json::Value {
    serde_json::json!({
        "call_id": call_id,
        "status": "failure",
        "message": message,
    })
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
    let control = prepare_runtime_control(prefix, request)?;
    let output = RuntimeOutputSpool::create(&control.path)?;
    let (stdout, stderr) = output.stdio()?;
    let (mut command, python) = runtime_command(&control)?;
    command
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
    read_runtime_response(&control).map_err(Into::into)
}

fn exchange_test_request<R: DeserializeOwned>(
    prefix: &str,
    request: &impl Serialize,
    log: &mut OperationLog,
    working_directory: &Path,
    test_report_path: &Path,
    callback: Arc<dyn TestControlCallback>,
) -> Result<RuntimeResponse<R>, ExchangeError> {
    let control = prepare_runtime_control(prefix, request)?;
    let (mut diagnostics, diagnostic_writer) =
        io::pipe().map_err(RuntimeInfrastructureError::CreateRuntimeOutputPipe)?;
    let (mut command, python) = runtime_command(&control)?;
    command
        .arg("--test-report")
        .arg(test_report_path)
        .current_dir(working_directory)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(diagnostic_writer);
    let mut child = command
        .spawn()
        .map_err(|source| RuntimeInfrastructureError::StartHost { python, source })?;
    drop(command);
    let stdin = child
        .stdin
        .take()
        .ok_or(RuntimeInfrastructureError::MissingHostStdin)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(RuntimeInfrastructureError::MissingHostStdout)?;
    let child = Arc::new(Mutex::new(child));
    let pump_child = Arc::clone(&child);
    let pump_result = thread::scope(|scope| {
        let pump = scope.spawn(move || {
            let result = pump_test_requests(
                BufReader::new(stdout),
                Arc::new(Mutex::new(stdin)),
                callback,
            );
            if result.is_err()
                && let Ok(mut child) = pump_child.lock()
            {
                let _ = child.kill();
            }
            result
        });
        let mirror_result = mirror_test_runtime_output(&mut diagnostics, log);
        if mirror_result.is_err()
            && let Ok(mut child) = child.lock()
        {
            let _ = child.kill();
        }
        let pump_result = pump
            .join()
            .unwrap_or(Err(RuntimeInfrastructureError::TestControlPumpPanicked));
        (mirror_result, pump_result)
    });
    let status = {
        let mut child = child
            .lock()
            .map_err(|_| RuntimeInfrastructureError::TestRuntimeHandlePoisoned)?;
        match child.wait() {
            Ok(status) => status,
            Err(source) => {
                terminate_test_runtime(&mut child);
                return Err(RuntimeInfrastructureError::WaitHost(source).into());
            }
        }
    };
    pump_result.0?;
    pump_result.1?;
    if !status.success() {
        return Err(RuntimeInfrastructureError::HostExit(status.code()).into());
    }
    let response = read_runtime_response(&control)?;
    serde_json::from_slice(&response).map_err(|source| {
        ExchangeError::InvalidResponse(format!("Runtime Response decoding failed: {source}"))
    })
}

struct RuntimeControl {
    _directory: tempfile::TempDir,
    path: PathBuf,
    request_path: PathBuf,
    response_path: PathBuf,
}

fn prepare_runtime_control(
    prefix: &str,
    request: &impl Serialize,
) -> Result<RuntimeControl, RuntimeInfrastructureError> {
    let directory = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .map_err(RuntimeInfrastructureError::ControlDirectory)?;
    let path =
        dunce::canonicalize(directory.path()).map_err(RuntimeInfrastructureError::ControlPath)?;
    let request_path = path.join("request.json");
    let response_path = path.join("response.json");
    let request = serde_json::to_vec(request).map_err(RuntimeInfrastructureError::EncodeRequest)?;
    fs::write(&request_path, request).map_err(RuntimeInfrastructureError::WriteRequest)?;
    Ok(RuntimeControl {
        _directory: directory,
        path,
        request_path,
        response_path,
    })
}

fn runtime_command(
    control: &RuntimeControl,
) -> Result<(Command, PathBuf), RuntimeInfrastructureError> {
    let python = bundled_python_path()?;
    if !python.is_file() {
        return Err(RuntimeInfrastructureError::MissingHost(python));
    }
    let mut command = Command::new(&python);
    command
        .args(["-I", "-B", "-X", "utf8", "-u", "-m", PRIVATE_RUNTIME_MODULE])
        .arg("--request")
        .arg(&control.request_path)
        .arg("--response")
        .arg(&control.response_path);
    Ok((command, python))
}

fn read_runtime_response(control: &RuntimeControl) -> Result<Vec<u8>, RuntimeInfrastructureError> {
    fs::read(&control.response_path).map_err(RuntimeInfrastructureError::ReadResponse)
}

fn mirror_test_runtime_output(
    output: &mut impl Read,
    log: &mut OperationLog,
) -> Result<(), ExchangeError> {
    let stderr = io::stderr();
    let mut terminal = stderr.lock();
    let mut mirror = RuntimeOutputMirror::new(log, &mut terminal);
    (|| {
        let mut buffer = [0_u8; 8192];
        loop {
            let count = output
                .read(&mut buffer)
                .map_err(RuntimeInfrastructureError::ReadPipedOutput)?;
            if count == 0 {
                break;
            }
            mirror.append(&buffer[..count])?;
        }
        mirror.finish()
    })()
}

fn terminate_test_runtime(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
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

#[derive(Debug, Error, Diagnostic)]
pub(crate) enum QueryRunError {
    #[error("Query Operation log could not be delivered")]
    #[diagnostic(help("Provide a writable KAT Data Home and retry the complete Query"))]
    OperationLog {
        #[source]
        source: OperationLogError,
        log_path: Option<String>,
    },
    #[error("Query Operation log is incomplete")]
    #[diagnostic(help(
        "Inspect the partial log if present, then provide writable storage and retry"
    ))]
    IncompleteOperationLog {
        #[source]
        source: OperationLogError,
        log_path: String,
    },
    #[error("Workflow Runtime query failed")]
    #[diagnostic(help(
        "Inspect the Operation log, narrow the query, correct its inputs, and retry"
    ))]
    Runtime {
        #[source]
        source: RuntimeInfrastructureError,
        log_path: String,
    },
}

impl QueryRunError {
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

impl RuntimeOperationError for QueryRunError {
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
    #[error("Bundled Python Host stdin control pipe is unavailable")]
    MissingHostStdin,
    #[error("Bundled Python Host stdout control pipe is unavailable")]
    MissingHostStdout,
    #[error("failed to read a nested Workflow control request")]
    ReadNestedRequest(#[source] io::Error),
    #[error("failed to start the nested Workflow request reader")]
    StartNestedRequestReader(#[source] io::Error),
    #[error("nested Workflow request reader stopped unexpectedly")]
    NestedRequestReaderPanicked,
    #[error("nested Workflow control pump stopped unexpectedly")]
    NestedControlPumpStopped,
    #[error("nested Workflow control request is invalid")]
    InvalidNestedRequest,
    #[error("failed to start a nested Workflow worker")]
    StartNestedWorker(#[source] io::Error),
    #[error("nested Workflow worker stopped unexpectedly")]
    NestedWorkerPanicked,
    #[error("failed to encode a nested Workflow control response")]
    EncodeNestedResponse(#[source] serde_json::Error),
    #[error("nested Workflow response writer is unavailable")]
    NestedResponseWriterPoisoned,
    #[error("failed to write a nested Workflow control response")]
    WriteNestedResponse(#[source] io::Error),
    #[error("PACK test control pump stopped unexpectedly")]
    TestControlPumpPanicked,
    #[error("PACK test Runtime process handle is unavailable")]
    TestRuntimeHandlePoisoned,
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
    #[error("failed to create the Runtime diagnostic output pipe")]
    CreateRuntimeOutputPipe(#[source] io::Error),
    #[error("failed to read piped Runtime diagnostic output")]
    ReadPipedOutput(#[source] io::Error),
    #[error("failed to mirror Runtime output to the terminal")]
    MirrorRuntimeOutput(#[source] io::Error),
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
    use super::protocol::NestedScalar;
    use std::{
        collections::BTreeMap,
        io::Cursor,
        sync::{Arc, Condvar, Mutex},
        thread,
        time::Duration,
    };

    use super::*;

    #[test]
    fn runtime_response_is_structurally_closed() {
        let valid_failure =
            br#"{"status":"failure","error":{"message":"Runtime Request is invalid"}}"#;
        assert!(matches!(
            serde_json::from_slice::<RuntimeResponse<InspectWorkflowsResult>>(valid_failure)
                .unwrap(),
            RuntimeResponse::Failure { .. }
        ));

        for invalid in [
            br#"{"status":"success","result":{"workflows":[],"extra":true}}"#.as_slice(),
            br#"{"status":"failure","failure_owner":"pack","error":{"message":"failed"}}"#
                .as_slice(),
            br#"{"status":"failure","error":{"message":"failed"},"extra":true}"#.as_slice(),
            br#"{"status":"success","result":{"workflows":[{"name":"w","description":"W.","title":"W"}]}}"#.as_slice(),
        ] {
            assert!(
                serde_json::from_slice::<RuntimeResponse<InspectWorkflowsResult>>(invalid)
                    .is_err()
            );
        }

        for invalid in [
            br#"{"status":"success","result":{"workflow":{"name":"w","description":"W.","parameters":[]}}}"#.as_slice(),
            br#"{"status":"success","result":{"workflow":{"name":"w","description":"W.","parameters":[],"guide":null,"extra":true}}}"#.as_slice(),
            br#"{"status":"success","result":{"workflow":{"name":"w","description":"W.","parameters":[{"name":"value","option":"--value","type":"string","required":false,"description":"Value","default":[] }],"guide":null}}}"#.as_slice(),
            br#"{"status":"success","result":{"workflow":{"name":"w","description":"W.","parameters":[{"name":"value","option":"--value","type":"string","required":false,"description":"Value","default":{} }],"guide":null}}}"#.as_slice(),
            br#"{"status":"success","result":{"workflow":{"name":"w","description":"W.","parameters":[{"name":"value","option":"--value","type":"path","required":true,"description":"Value"}],"guide":null}}}"#.as_slice(),
        ] {
            assert!(
                serde_json::from_slice::<RuntimeResponse<InspectWorkflowResult>>(invalid).is_err()
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
                r#"{{"status":"success","result":{{"workflow":{{"name":"w","description":"W.","parameters":[{{"name":"value","option":"--value","type":"{parameter_type}","required":true,"description":"Value"}}],"guide":null}}}}}}"#
            );
            assert!(
                serde_json::from_str::<RuntimeResponse<InspectWorkflowResult>>(&response).is_ok(),
                "parameter type should be part of the closed set: {parameter_type}"
            );
        }
    }

    #[test]
    fn runtime_response_accepts_only_scalar_parameter_defaults() {
        for default in [r#""value""#, "42", "1.5", "true", "null"] {
            let response = format!(
                r#"{{"status":"success","result":{{"workflow":{{"name":"w","description":"W.","parameters":[{{"name":"value","option":"--value","type":"string","required":false,"description":"Value","default":{default}}}],"guide":null}}}}}}"#
            );
            assert!(
                serde_json::from_str::<RuntimeResponse<InspectWorkflowResult>>(&response).is_ok(),
                "default should be a valid JSON scalar: {default}"
            );
        }
    }

    #[test]
    fn run_workflow_requests_keep_cli_arguments_and_nested_inputs_separate() {
        let invocation = RunWorkflowInvocation {
            session_id: "019f6e00-0000-7000-8000-000000000000".to_owned(),
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            input: WorkflowInputs::Arguments(Vec::new()),
            candidate_id: "019f6e00-0000-7000-8000-000000000001".to_owned(),
            candidate_path: "C:\\data\\runs\\019f6e00-0000-7000-8000-000000000001".to_owned(),
            datasource_root: "C:\\data\\datasources\\example".to_owned(),
            scratch_root: "C:\\data\\scratch\\candidate".to_owned(),
        };

        let request = serde_json::to_value(run_workflow_runtime_request(&invocation)).unwrap();

        assert_eq!(
            request,
            serde_json::json!({
                "operation": "run_workflow",
                "pack_name": "example",
                "pack_path": "C:\\pack",
                "workflow_name": "analyze",
                "input": {"kind": "arguments", "value": []},
                "candidate_id": "019f6e00-0000-7000-8000-000000000001",
                "candidate_path": "C:\\data\\runs\\019f6e00-0000-7000-8000-000000000001",
                "datasource_root": "C:\\data\\datasources\\example",
                "scratch_root": "C:\\data\\scratch\\candidate"
            })
        );
        assert!(request.get("session_id").is_none());

        let inputs = BTreeMap::from([("limit".to_owned(), NestedScalar::Int64("5".to_owned()))]);
        let invocation = RunWorkflowInvocation {
            input: WorkflowInputs::TypedInputs(inputs),
            ..invocation
        };
        let request = serde_json::to_value(run_workflow_runtime_request(&invocation)).unwrap();
        assert_eq!(
            request,
            serde_json::json!({
                "operation": "run_workflow",
                "pack_name": "example",
                "pack_path": "C:\\pack",
                "workflow_name": "analyze",
                "input": {"kind": "typed_inputs", "value": {"limit": {"type": "int64", "value": "5"}}},
                "candidate_id": "019f6e00-0000-7000-8000-000000000001",
                "candidate_path": "C:\\data\\runs\\019f6e00-0000-7000-8000-000000000001",
                "datasource_root": "C:\\data\\datasources\\example",
                "scratch_root": "C:\\data\\scratch\\candidate"
            })
        );
        assert!(request.get("arguments").is_none());
    }

    #[test]
    fn run_runtime_diagnostic_rejects_private_runtime_values() {
        let invocation = RunWorkflowInvocation {
            session_id: "019f6e00-0000-7000-8000-000000000000".to_owned(),
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            input: WorkflowInputs::Arguments(Vec::new()),
            candidate_id: "019f6e00-0000-7000-8000-000000000001".to_owned(),
            candidate_path: "C:\\private\\019f6e00-0000-7000-8000-000000000001".to_owned(),
            datasource_root: "C:\\data\\datasources\\example".to_owned(),
            scratch_root: "C:\\data\\scratch\\candidate".to_owned(),
        };
        for private in [
            invocation.session_id.clone(),
            invocation.candidate_id.clone(),
            invocation.candidate_path.clone(),
            invocation.candidate_path.replace('\\', "/"),
            invocation.datasource_root.clone(),
            invocation.datasource_root.replace('\\', "/"),
            invocation.scratch_root.clone(),
            invocation.scratch_root.replace('\\', "/"),
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
    fn run_success_rejects_private_runtime_values_before_the_result_becomes_trusted() {
        let invocation = RunWorkflowInvocation {
            session_id: "019f6e00-0000-7000-8000-000000000000".to_owned(),
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            input: WorkflowInputs::Arguments(Vec::new()),
            candidate_id: "019f6e00-0000-7000-8000-000000000001".to_owned(),
            candidate_path: "C:\\private\\019f6e00-0000-7000-8000-000000000001".to_owned(),
            datasource_root: "C:\\data\\datasources\\example".to_owned(),
            scratch_root: "C:\\data\\scratch\\candidate".to_owned(),
        };
        for private in [
            invocation.candidate_id.clone(),
            invocation.candidate_path.clone(),
            invocation.candidate_path.replace('\\', "/"),
            invocation.datasource_root.clone(),
            invocation.datasource_root.replace('\\', "/"),
            invocation.scratch_root.clone(),
            invocation.scratch_root.replace('\\', "/"),
        ] {
            let response = serde_json::to_vec(&serde_json::json!({
                "status": "success",
                "result": {
                    "effective_inputs": {"value": private},
                    "outputs": {
                        "main": {
                            "columns": [{"name": "value", "type": "int64"}],
                            "row_count": 0
                        }
                    }
                }
            }))
            .unwrap();

            assert!(decode_and_validate_run_workflow_response(&response, &invocation).is_err());
        }
    }

    #[test]
    fn run_success_accepts_the_public_session_id() {
        let invocation = RunWorkflowInvocation {
            session_id: "019f6e00-0000-7000-8000-000000000000".to_owned(),
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            input: WorkflowInputs::Arguments(Vec::new()),
            candidate_id: "019f6e00-0000-7000-8000-000000000001".to_owned(),
            candidate_path: "C:\\private\\019f6e00-0000-7000-8000-000000000001".to_owned(),
            datasource_root: "C:\\data\\datasources\\example".to_owned(),
            scratch_root: "C:\\data\\scratch\\candidate".to_owned(),
        };
        let response = serde_json::to_vec(&serde_json::json!({
            "status": "success",
            "result": {
                "effective_inputs": {"session": invocation.session_id},
                "outputs": {
                    "main": {
                        "columns": [{"name": "value", "type": "int64"}],
                        "row_count": 0
                    }
                }
            }
        }))
        .unwrap();

        assert!(decode_and_validate_run_workflow_response(&response, &invocation).is_ok());
    }

    #[test]
    fn run_success_accepts_named_output_without_a_second_identity() {
        let candidate = tempfile::tempdir().unwrap();
        let output_root = candidate.path().join("outputs");
        fs::create_dir(&output_root).unwrap();
        fs::write(output_root.join("main.parquet"), b"opaque output").unwrap();
        let invocation = RunWorkflowInvocation {
            session_id: "019f6e00-0000-7000-8000-000000000000".to_owned(),
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            input: WorkflowInputs::Arguments(Vec::new()),
            candidate_id: "019f6e00-0000-7000-8000-000000000002".to_owned(),
            candidate_path: candidate.path().to_str().unwrap().to_owned(),
            datasource_root: "C:\\data\\datasources\\example".to_owned(),
            scratch_root: "C:\\data\\scratch\\candidate".to_owned(),
        };
        let response = br#"{"status":"success","result":{"effective_inputs":{},"outputs":{"main":{"columns":[{"name":"value","type":"int64"}],"row_count":0}}}}"#;

        assert!(decode_and_validate_run_workflow_response(response, &invocation).is_ok());
    }

    #[test]
    fn run_success_does_not_reinspect_runtime_owned_output_files() {
        let candidate = tempfile::tempdir().unwrap();
        let invocation = RunWorkflowInvocation {
            session_id: "019f6e00-0000-7000-8000-000000000000".to_owned(),
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            input: WorkflowInputs::Arguments(Vec::new()),
            candidate_id: "019f6e00-0000-7000-8000-000000000002".to_owned(),
            candidate_path: candidate.path().to_str().unwrap().to_owned(),
            datasource_root: "C:\\data\\datasources\\example".to_owned(),
            scratch_root: "C:\\data\\scratch\\candidate".to_owned(),
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
            session_id: "019f6e00-0000-7000-8000-000000000000".to_owned(),
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            input: WorkflowInputs::Arguments(Vec::new()),
            candidate_id: "019f6e00-0000-7000-8000-000000000003".to_owned(),
            candidate_path: "C:\\private\\019f6e00-0000-7000-8000-000000000003".to_owned(),
            datasource_root: "C:\\data\\datasources\\example".to_owned(),
            scratch_root: "C:\\data\\scratch\\candidate".to_owned(),
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
            session_id: "019f6e00-0000-7000-8000-000000000000".to_owned(),
            pack_name: "example".to_owned(),
            pack_path: "C:\\pack".to_owned(),
            workflow_name: "analyze".to_owned(),
            input: WorkflowInputs::Arguments(Vec::new()),
            candidate_id: "019f6e00-0000-7000-8000-000000000002".to_owned(),
            candidate_path: candidate.path().to_str().unwrap().to_owned(),
            datasource_root: "C:\\data\\datasources\\example".to_owned(),
            scratch_root: "C:\\data\\scratch\\candidate".to_owned(),
        };
        assert!(validate_run_workflow_report(result(BTreeMap::new()), &invocation).is_ok());
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

    struct OrderingCallback {
        fast_started: (Mutex<bool>, Condvar),
    }

    impl NestedRunCallback for OrderingCallback {
        fn execute(&self, call: protocol::NestedRunCall) -> protocol::NestedRunOutcome {
            if call.pack_name == "slow" {
                let (lock, ready) = &self.fast_started;
                let started = lock.lock().unwrap();
                let (started, timeout) = ready
                    .wait_timeout_while(started, Duration::from_secs(2), |started| !*started)
                    .unwrap();
                assert!(
                    !timeout.timed_out(),
                    "request reader blocked behind callback"
                );
                assert!(*started);
                thread::sleep(Duration::from_millis(25));
            } else {
                let (lock, ready) = &self.fast_started;
                *lock.lock().unwrap() = true;
                ready.notify_all();
            }
            protocol::NestedRunOutcome::Success {
                relations: vec![protocol::NestedRelation {
                    name: "main".to_owned(),
                    path: format!("C:\\private\\{}\\main.parquet", call.pack_name),
                }],
            }
        }
    }

    #[test]
    fn nested_request_pump_dispatches_concurrently_and_writes_whole_frames() {
        let requests = concat!(
            r#"{"call_id":1,"pack_name":"slow","workflow_name":"run","inputs":{}}"#,
            "\n",
            r#"{"call_id":2,"pack_name":"fast","workflow_name":"run","inputs":{}}"#,
            "\n"
        );
        let output = Arc::new(Mutex::new(Vec::new()));
        let callback = Arc::new(OrderingCallback {
            fast_started: (Mutex::new(false), Condvar::new()),
        });

        pump_nested_requests(
            Cursor::new(requests.as_bytes()),
            Arc::clone(&output),
            callback,
        )
        .unwrap();

        let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        let frames = output
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0]["call_id"], 2);
        assert_eq!(frames[1]["call_id"], 1);
        assert!(frames.iter().all(|frame| frame["status"] == "success"));
    }

    #[derive(Default)]
    struct BlockingReaderState {
        released: bool,
        finished: bool,
    }

    struct BlockingAfterFrameReader {
        frame: Cursor<Vec<u8>>,
        state: Arc<(Mutex<BlockingReaderState>, Condvar)>,
    }

    impl Read for BlockingAfterFrameReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.frame.position() < self.frame.get_ref().len() as u64 {
                return self.frame.read(buffer);
            }
            let (state, ready) = &*self.state;
            let mut state = ready
                .wait_while(state.lock().unwrap(), |state| !state.released)
                .unwrap();
            state.finished = true;
            ready.notify_all();
            Ok(0)
        }
    }

    struct FailingResponseWriter;

    impl Write for FailingResponseWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "injected response write failure",
            ))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn nested_response_write_failure_does_not_wait_for_request_eof() {
        let request = concat!(
            r#"{"call_id":1,"pack_name":"child","workflow_name":"run","inputs":{}}"#,
            "\n"
        );
        let reader_state = Arc::new((Mutex::new(BlockingReaderState::default()), Condvar::new()));
        let reader = BlockingAfterFrameReader {
            frame: Cursor::new(request.as_bytes().to_vec()),
            state: Arc::clone(&reader_state),
        };
        let callback = Arc::new(OrderingCallback {
            fast_started: (Mutex::new(true), Condvar::new()),
        });
        let (result_sender, result_receiver) = mpsc::channel();
        let pump = thread::spawn(move || {
            let result = pump_nested_requests(
                BufReader::new(reader),
                Arc::new(Mutex::new(FailingResponseWriter)),
                callback,
            );
            let _ = result_sender.send(result);
        });

        let early_result = result_receiver.recv_timeout(Duration::from_secs(2));
        let (state, ready) = &*reader_state;
        {
            let mut state = state.lock().unwrap();
            state.released = true;
            ready.notify_all();
        }
        pump.join().unwrap();
        let (state, timeout) = ready
            .wait_timeout_while(state.lock().unwrap(), Duration::from_secs(2), |state| {
                !state.finished
            })
            .unwrap();

        assert!(!timeout.timed_out(), "request reader did not stop");
        assert!(state.finished);
        assert!(matches!(
            early_result.expect("response write failure waited for request EOF"),
            Err(RuntimeInfrastructureError::WriteNestedResponse(_))
        ));
    }

    #[test]
    fn nested_request_pump_rejects_reused_call_ids_and_partial_frames() {
        let duplicate = concat!(
            r#"{"call_id":1,"pack_name":"one","workflow_name":"run","inputs":{}}"#,
            "\n",
            r#"{"call_id":1,"pack_name":"two","workflow_name":"run","inputs":{}}"#,
            "\n"
        );
        let callback = Arc::new(OrderingCallback {
            fast_started: (Mutex::new(true), Condvar::new()),
        });
        assert!(matches!(
            pump_nested_requests(
                Cursor::new(duplicate.as_bytes()),
                Arc::new(Mutex::new(Vec::new())),
                Arc::clone(&callback),
            ),
            Err(RuntimeInfrastructureError::InvalidNestedRequest)
        ));

        let partial = r#"{"call_id":3,"pack_name":"one","workflow_name":"run","inputs":{}}"#;
        assert!(matches!(
            pump_nested_requests(
                Cursor::new(partial.as_bytes()),
                Arc::new(Mutex::new(Vec::new())),
                callback,
            ),
            Err(RuntimeInfrastructureError::InvalidNestedRequest)
        ));
    }
}
