use std::{
    collections::HashSet,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::LazyLock,
    sync::mpsc::{self, Sender},
    thread,
};

use miette::Diagnostic;
use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    operation_log::{OperationLog, OperationLogError},
    response::KatDiagnostic,
    text_projection::TextProjection,
};

const PRIVATE_RUNTIME_MODULE: &str = "_kat_runtime";
static WORKFLOW_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A[a-z0-9]+(?:-[a-z0-9]+)*\z").unwrap());
static TABLE_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A[a-z][a-z0-9]*(?:_[a-z0-9]+)*\z").unwrap());
static DURATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A(?<whole>[0-9]+)(?:\.(?<fraction>[0-9]{1,9}))?(?<unit>ns|us|ms|s|min|h)\z")
        .unwrap()
});

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

pub(crate) fn inspect_pack(
    mut log: OperationLog,
    pack_name: &str,
    pack_path: &Path,
) -> Result<InspectPackOutcome, InspectPackError> {
    let response = match exchange(pack_name, pack_path, &mut log) {
        Ok(response) => response,
        Err(ExchangeError::Log(error)) => return Err(InspectPackError::operation_log(error)),
        Err(ExchangeError::Runtime(error)) => return Err(finish_runtime_error(log, error)),
    };
    match response {
        RuntimeResponse::Success { result } => {
            if let Err(error) = validate_workflows(&result.workflows) {
                return Err(finish_runtime_error(log, error));
            }
            if let Err(source) = log.append(b"status: success\n") {
                return Err(InspectPackError::operation_log(source));
            }
            let log_path = log.finish().map_err(InspectPackError::operation_log)?;
            Ok(InspectPackOutcome::Success {
                workflows: result.workflows,
                log_path,
            })
        }
        RuntimeResponse::Failure { error } => {
            if !error.validate() {
                return Err(finish_runtime_error(
                    log,
                    RuntimeFailure::InvalidResponse(
                        "Runtime Diagnostic contains empty or invalid fields".to_owned(),
                    ),
                ));
            }
            if let Err(source) = log.append(b"status: failure\n") {
                return Err(InspectPackError::operation_log(source));
            }
            let log_path = log.finish().map_err(InspectPackError::operation_log)?;
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

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum RuntimeResponse {
    Success { result: InspectPackResult },
    Failure { error: KatDiagnostic },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectPackResult {
    workflows: Vec<Workflow>,
}

fn exchange(
    pack_name: &str,
    pack_path: &Path,
    log: &mut OperationLog,
) -> Result<RuntimeResponse, ExchangeError> {
    let pack_path_text = pack_path
        .to_str()
        .ok_or_else(|| RuntimeFailure::NonUnicodePackPath(pack_path.to_path_buf()))?;
    let control = tempfile::Builder::new()
        .prefix("kat-inspect-pack-")
        .tempdir()
        .map_err(RuntimeFailure::ControlDirectory)?;
    let control_path = dunce::canonicalize(control.path()).map_err(RuntimeFailure::ControlPath)?;
    let request_path = control_path.join("request.json");
    let response_path = control_path.join("response.json");
    let request = serde_json::to_vec(&InspectPackRequest {
        operation: "inspect_pack",
        pack_name,
        pack_path: pack_path_text,
    })
    .map_err(RuntimeFailure::EncodeRequest)?;
    fs::write(&request_path, request).map_err(RuntimeFailure::WriteRequest)?;

    let python = bundled_python_path()?;
    if !python.is_file() {
        return Err(RuntimeFailure::MissingHost(python).into());
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
        .map_err(|source| RuntimeFailure::StartHost { python, source })?;

    if let Err(error) = capture_streams(&mut child, log) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let status = child.wait().map_err(RuntimeFailure::WaitHost)?;
    if !status.success() {
        return Err(RuntimeFailure::HostExit(status.code()).into());
    }
    let response = fs::read(&response_path).map_err(RuntimeFailure::ReadResponse)?;
    serde_json::from_slice(&response)
        .map_err(RuntimeFailure::DecodeResponse)
        .map_err(Into::into)
}

fn bundled_python_path() -> Result<PathBuf, RuntimeFailure> {
    let executable = std::env::current_exe().map_err(RuntimeFailure::CurrentExecutable)?;
    let payload = executable
        .parent()
        .ok_or_else(|| RuntimeFailure::InvalidPayload(executable.clone()))?;
    #[cfg(windows)]
    let python = payload.join("python").join("python.exe");
    #[cfg(not(windows))]
    let python = payload.join("python").join("bin").join("python3");
    Ok(python)
}

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

enum StreamEvent {
    Bytes(Stream, Vec<u8>),
    Error(Stream, io::Error),
    Finished(Stream),
}

trait RuntimeLogSink {
    fn append(&mut self, bytes: &[u8]) -> Result<(), OperationLogError>;
}

impl RuntimeLogSink for OperationLog {
    fn append(&mut self, bytes: &[u8]) -> Result<(), OperationLogError> {
        OperationLog::append(self, bytes)
    }
}

fn capture_streams(child: &mut Child, log: &mut impl RuntimeLogSink) -> Result<(), ExchangeError> {
    let stdout = child
        .stdout
        .take()
        .ok_or(RuntimeFailure::MissingPipe("stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or(RuntimeFailure::MissingPipe("stderr"))?;
    let (sender, receiver) = mpsc::channel();
    let stdout_thread = spawn_stream_reader(stdout, Stream::Stdout, sender.clone());
    let stderr_thread = spawn_stream_reader(stderr, Stream::Stderr, sender);
    let mut stdout_projection = TextProjection::new("stdout");
    let mut stderr_projection = TextProjection::new("stderr");
    let mut finished = 0;
    while finished < 2 {
        let event = receiver.recv().map_err(|_| RuntimeFailure::StreamChannel)?;
        match event {
            StreamEvent::Bytes(stream, bytes) => {
                let projection = match stream {
                    Stream::Stdout => &mut stdout_projection,
                    Stream::Stderr => &mut stderr_projection,
                };
                let text = projection.push(&bytes);
                if let Err(error) = log.append(text.as_bytes()) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ExchangeError::Log(error));
                }
            }
            StreamEvent::Error(stream, source) => {
                let error = RuntimeFailure::ReadStream {
                    stream: stream.name(),
                    source,
                };
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
            StreamEvent::Finished(stream) => {
                finished += 1;
                let projection = match stream {
                    Stream::Stdout => &mut stdout_projection,
                    Stream::Stderr => &mut stderr_projection,
                };
                let text = projection.finish();
                if let Err(error) = log.append(text.as_bytes()) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ExchangeError::Log(error));
                }
            }
        }
    }
    stdout_thread
        .join()
        .map_err(|_| RuntimeFailure::StreamThread("stdout"))?;
    stderr_thread
        .join()
        .map_err(|_| RuntimeFailure::StreamThread("stderr"))?;
    Ok(())
}

impl Stream {
    fn name(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

fn spawn_stream_reader(
    mut reader: impl Read + Send + 'static,
    stream: Stream,
    sender: Sender<StreamEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut plain = strip_ansi::StripStream::new();
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let mut stripped = Vec::with_capacity(count);
                    plain.push(&buffer[..count], &mut stripped);
                    if sender.send(StreamEvent::Bytes(stream, stripped)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(StreamEvent::Error(stream, error));
                    break;
                }
            }
        }
        plain.finish();
        let _ = sender.send(StreamEvent::Finished(stream));
    })
}

fn validate_workflows(workflows: &[Workflow]) -> Result<(), RuntimeFailure> {
    let mut previous_name: Option<&str> = None;
    for workflow in workflows {
        if !WORKFLOW_NAME.is_match(&workflow.name) {
            return invalid_response(format!("invalid Workflow name {:?}", workflow.name));
        }
        normalized_non_empty(&workflow.title, "Workflow title")?;
        normalized_non_empty(&workflow.description, "Workflow description")?;
        if previous_name.is_some_and(|previous| previous >= workflow.name.as_str()) {
            return Err(RuntimeFailure::InvalidResponse(
                "Workflow names must be strictly sorted and unique".to_owned(),
            ));
        }
        previous_name = Some(&workflow.name);
        if !strictly_sorted_unique(&workflow.required_tables) {
            return Err(RuntimeFailure::InvalidResponse(
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
                return Err(RuntimeFailure::InvalidResponse(format!(
                    "duplicate Workflow parameter {:?}",
                    parameter.name
                )));
            }
            validate_parameter(parameter)?;
        }
    }
    Ok(())
}

fn validate_parameter(parameter: &Parameter) -> Result<(), RuntimeFailure> {
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
        return Err(RuntimeFailure::InvalidResponse(format!(
            "unsupported parameter type {:?}",
            parameter.parameter_type
        )));
    }
    if (parameter.parameter_type == "boolean") != parameter.negative_option.is_some() {
        return Err(RuntimeFailure::InvalidResponse(
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
        return Err(RuntimeFailure::InvalidResponse(
            "choices must be a non-empty sorted unique string set".to_owned(),
        ));
    }
    if parameter.required != parameter.default.is_missing() {
        return Err(RuntimeFailure::InvalidResponse(
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
            return Err(RuntimeFailure::InvalidResponse(
                "parameter default does not match its public type".to_owned(),
            ));
        }
        if let (Some(choices), serde_json::Value::String(value)) = (&parameter.choices, default)
            && choices.binary_search(value).is_err()
        {
            return Err(RuntimeFailure::InvalidResponse(
                "string Literal default must be one of its choices".to_owned(),
            ));
        }
    }
    Ok(())
}

fn non_empty(value: &str, label: &str) -> Result<(), RuntimeFailure> {
    if value.trim().is_empty() {
        return Err(RuntimeFailure::InvalidResponse(format!(
            "{label} must not be empty"
        )));
    }
    Ok(())
}

fn normalized_non_empty(value: &str, label: &str) -> Result<(), RuntimeFailure> {
    non_empty(value, label)?;
    if value != value.trim() {
        return invalid_response(format!("{label} must not contain outer whitespace"));
    }
    Ok(())
}

fn invalid_response<T>(message: String) -> Result<T, RuntimeFailure> {
    Err(RuntimeFailure::InvalidResponse(message))
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

fn finish_runtime_error(mut log: OperationLog, error: RuntimeFailure) -> InspectPackError {
    let details = format!("status: failure\nerror: {error}\n");
    if let Err(log_error) = log.append(details.as_bytes()) {
        return InspectPackError::operation_log(log_error);
    }
    match log.finish() {
        Ok(log_path) => InspectPackError::Runtime {
            source: error,
            log_path,
        },
        Err(log_error) => InspectPackError::operation_log(log_error),
    }
}

enum ExchangeError {
    Log(OperationLogError),
    Runtime(RuntimeFailure),
}

impl From<RuntimeFailure> for ExchangeError {
    fn from(error: RuntimeFailure) -> Self {
        Self::Runtime(error)
    }
}

#[derive(Debug, Error, Diagnostic)]
pub(crate) enum InspectPackError {
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
        source: RuntimeFailure,
        log_path: String,
    },
}

impl InspectPackError {
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
pub(crate) enum RuntimeFailure {
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
    use std::io::Write as _;
    use std::time::{Duration, Instant};

    struct FailingLog {
        path: PathBuf,
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
        assert!(serde_json::from_slice::<RuntimeResponse>(unknown).is_err());

        let response = br#"{"status":"success","result":{"workflows":[{"name":"a","title":"A","description":"A","required_tables":[],"parameters":[{"name":"flag","option":"--flag","type":"boolean","required":false,"description":"Flag","default":false}]}]}}"#;
        let RuntimeResponse::Success { result } =
            serde_json::from_slice::<RuntimeResponse>(response).unwrap()
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
                serde_json::from_value::<RuntimeResponse>(response).unwrap()
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

        let error = InspectPackError::operation_log(OperationLogError::Flush {
            path,
            source: io::Error::other("injected flush failure"),
        });

        assert_eq!(
            error.to_string(),
            "PACK inspection Operation log is incomplete"
        );
        assert!(matches!(
            error,
            InspectPackError::IncompleteOperationLog { .. }
        ));
    }

    #[test]
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
            Ok("descendant") => thread::sleep(Duration::from_secs(3)),
            _ => {}
        }
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
