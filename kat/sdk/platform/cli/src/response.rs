use std::{
    error::Error,
    fmt::{self, Display},
    fs,
    io::Write,
    path::PathBuf,
    process::ExitCode,
};

use miette::{
    Diagnostic, LabeledSpan, MietteError, MietteSpanContents, SourceCode, SourceSpan, SpanContents,
};
use serde::{Deserialize, Serialize};

use crate::session_store::SessionLease;
use crate::text_projection::project_complete_text;

pub(super) struct PreparedResponse<P> {
    response: KatResponse<P>,
    rendered_diagnostic: Option<RenderedDiagnostic>,
    exit_code: ExitCode,
    pending_file: Option<PendingResponseFile>,
    session_lease: Option<SessionLease>,
}

pub(super) struct PendingResponseFile {
    path: PathBuf,
    remove_on_drop: bool,
}

impl PendingResponseFile {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            remove_on_drop: true,
        }
    }

    fn retain(mut self) {
        self.remove_on_drop = false;
    }
}

impl Drop for PendingResponseFile {
    fn drop(&mut self) {
        if self.remove_on_drop && fs::remove_file(&self.path).is_err() {
            let _ = fs::remove_dir(&self.path);
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum KatResponse<P> {
    Success {
        result: P,
        #[serde(skip_serializing_if = "Option::is_none")]
        log_path: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        test_report_path: Option<String>,
    },
    Failure {
        error: KatDiagnostic,
        #[serde(skip_serializing_if = "Option::is_none")]
        log_path: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        test_report_path: Option<String>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct KatDiagnostic {
    message: String,
    #[serde(default, deserialize_with = "deserialize_non_empty_causes")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    causes: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_optional_nonnull")]
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_nonnull")]
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<DiagnosticLocation>,
}

fn deserialize_optional_nonnull<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

fn deserialize_non_empty_causes<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let causes = Vec::<String>::deserialize(deserializer)?;
    if causes.is_empty() {
        return Err(serde::de::Error::custom(
            "Runtime Diagnostic causes must be omitted or non-empty",
        ));
    }
    Ok(causes)
}

impl KatDiagnostic {
    pub(crate) fn reason(&self) -> String {
        std::iter::once(self.message.as_str())
            .chain(self.causes.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(": ")
    }

    pub(super) fn validate(&self) -> bool {
        !self.message.trim().is_empty()
            && self.causes.iter().all(|cause| !cause.trim().is_empty())
            && self
                .help
                .as_ref()
                .is_none_or(|help| !help.trim().is_empty())
            && self.location.as_ref().is_none_or(DiagnosticLocation::valid)
    }

    pub(super) fn contains_private_value(&self, value: &str) -> bool {
        !value.is_empty()
            && (self.message.contains(value)
                || self.causes.iter().any(|cause| cause.contains(value))
                || self.help.as_ref().is_some_and(|help| help.contains(value))
                || self
                    .location
                    .as_ref()
                    .is_some_and(|location| location.source.contains(value)))
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DiagnosticLocation {
    source: String,
    start: DiagnosticPosition,
    end: DiagnosticPosition,
}

impl DiagnosticLocation {
    fn valid(&self) -> bool {
        valid_runtime_location_source(&self.source)
            && self.start.line > 0
            && self.start.column > 0
            && self.end.line > 0
            && self.end.column > 0
            && (self.end.line, self.end.column) >= (self.start.line, self.start.column)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DiagnosticPosition {
    line: usize,
    column: usize,
}

struct RenderedDiagnostic(String);

pub(super) fn prepare_success<P>(result: P) -> PreparedResponse<P> {
    prepare_success_with_log(result, None)
}

pub(super) fn prepare_success_with_log<P>(
    result: P,
    log_path: Option<String>,
) -> PreparedResponse<P> {
    prepare_success_with_artifacts(result, log_path, None)
}

pub(super) fn prepare_success_with_log_and_file<P>(
    result: P,
    log_path: Option<String>,
    pending_file: PendingResponseFile,
) -> PreparedResponse<P> {
    let mut prepared = prepare_success_with_artifacts(result, log_path, None);
    prepared.pending_file = Some(pending_file);
    prepared
}

fn prepare_success_with_artifacts<P>(
    result: P,
    log_path: Option<String>,
    test_report_path: Option<String>,
) -> PreparedResponse<P> {
    PreparedResponse {
        response: KatResponse::Success {
            result,
            log_path,
            test_report_path,
        },
        rendered_diagnostic: None,
        exit_code: ExitCode::SUCCESS,
        pending_file: None,
        session_lease: None,
    }
}

pub(super) fn prepare_cli_failure<P>(report: miette::Report) -> PreparedResponse<P> {
    prepare_cli_failure_with_log(report, None)
}

pub(super) fn prepare_cli_failure_with_log<P>(
    report: miette::Report,
    log_path: Option<String>,
) -> PreparedResponse<P> {
    let diagnostic = cli_diagnostic(&report);
    let rendered_diagnostic = RenderedDiagnostic(format!("{report:?}"));

    PreparedResponse {
        response: KatResponse::Failure {
            error: diagnostic,
            log_path,
            test_report_path: None,
        },
        rendered_diagnostic: Some(rendered_diagnostic),
        exit_code: ExitCode::FAILURE,
        pending_file: None,
        session_lease: None,
    }
}

pub(super) fn prepare_runtime_failure<P>(
    diagnostic: KatDiagnostic,
    log_path: String,
) -> PreparedResponse<P> {
    prepare_runtime_failure_with_artifacts(diagnostic, log_path, None)
}

fn prepare_runtime_failure_with_artifacts<P>(
    diagnostic: KatDiagnostic,
    log_path: String,
    test_report_path: Option<String>,
) -> PreparedResponse<P> {
    let report = miette::Report::new(RuntimeDiagnosticPresentation::new(&diagnostic));
    let rendered_diagnostic = RenderedDiagnostic(project_complete_text(&format!("{report:?}")));
    PreparedResponse {
        response: KatResponse::Failure {
            error: diagnostic,
            log_path: Some(log_path),
            test_report_path,
        },
        rendered_diagnostic: Some(rendered_diagnostic),
        exit_code: ExitCode::FAILURE,
        pending_file: None,
        session_lease: None,
    }
}

pub(super) fn retain_session_lease<P>(
    mut prepared: PreparedResponse<P>,
    lease: SessionLease,
) -> PreparedResponse<P> {
    prepared.session_lease = Some(lease);
    prepared
}

pub(super) fn prepare_test_success<P>(
    result: P,
    log_path: String,
    test_report_path: String,
) -> PreparedResponse<P> {
    prepare_success_with_artifacts(result, Some(log_path), Some(test_report_path))
}

pub(super) fn prepare_test_runtime_failure<P>(
    diagnostic: KatDiagnostic,
    log_path: String,
    test_report_path: Option<String>,
) -> PreparedResponse<P> {
    prepare_runtime_failure_with_artifacts(diagnostic, log_path, test_report_path)
}

pub(super) fn prepare_test_cli_failure<P>(
    report: miette::Report,
    log_path: Option<String>,
    test_report_path: Option<String>,
) -> PreparedResponse<P> {
    let diagnostic = cli_diagnostic(&report);
    let rendered_diagnostic = RenderedDiagnostic(project_complete_text(&format!("{report:?}")));
    PreparedResponse {
        response: KatResponse::Failure {
            error: diagnostic,
            log_path,
            test_report_path,
        },
        rendered_diagnostic: Some(rendered_diagnostic),
        exit_code: ExitCode::FAILURE,
        pending_file: None,
        session_lease: None,
    }
}

fn cli_diagnostic(report: &miette::Report) -> KatDiagnostic {
    let diagnostic: &dyn Diagnostic = report.as_ref();
    let help = diagnostic
        .help()
        .map(|help| help.to_string())
        .filter(|help| !help.trim().is_empty());
    let mut causes = Vec::new();
    let mut source = std::error::Error::source(diagnostic);
    while let Some(cause) = source {
        let rendered = cause.to_string();
        if !rendered.trim().is_empty() {
            causes.push(rendered);
        }
        source = cause.source();
    }
    KatDiagnostic {
        message: diagnostic.to_string(),
        causes,
        help,
        location: project_location(diagnostic),
    }
}

#[derive(Debug)]
struct RuntimeDiagnosticPresentation {
    message: String,
    cause: Option<Box<RuntimeDiagnosticCause>>,
    help: Option<String>,
    location: Option<RuntimeDiagnosticLocation>,
}

impl RuntimeDiagnosticPresentation {
    fn new(diagnostic: &KatDiagnostic) -> Self {
        let cause = diagnostic
            .causes
            .iter()
            .rev()
            .fold(None, |source, message| {
                Some(Box::new(RuntimeDiagnosticCause {
                    message: message.clone(),
                    source,
                }))
            });
        Self {
            message: diagnostic.message.clone(),
            cause,
            help: diagnostic.help.clone(),
            location: diagnostic
                .location
                .as_ref()
                .map(RuntimeDiagnosticLocation::from),
        }
    }
}

fn valid_runtime_location_source(source: &str) -> bool {
    if source.is_empty()
        || source != source.trim()
        || source.starts_with('/')
        || source.starts_with('\\')
        || source.contains('\\')
        || source.chars().any(char::is_control)
    {
        return false;
    }
    let bytes = source.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return false;
    }
    source
        .split('/')
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

impl Display for RuntimeDiagnosticPresentation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for RuntimeDiagnosticPresentation {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_deref().map(|cause| cause as _)
    }
}

impl Diagnostic for RuntimeDiagnosticPresentation {
    fn help<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        self.help
            .as_ref()
            .map(|help| Box::new(help) as Box<dyn Display>)
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.location.as_ref().map(|location| location as _)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        self.location.as_ref()?;
        Some(Box::new(std::iter::once(
            LabeledSpan::new_primary_with_span(None, (0, 0)),
        )))
    }
}

#[derive(Debug)]
struct RuntimeDiagnosticCause {
    message: String,
    source: Option<Box<RuntimeDiagnosticCause>>,
}

impl Display for RuntimeDiagnosticCause {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for RuntimeDiagnosticCause {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref().map(|source| source as _)
    }
}

#[derive(Debug)]
struct RuntimeDiagnosticLocation {
    source: String,
    start: DiagnosticPosition,
    end: DiagnosticPosition,
}

impl From<&DiagnosticLocation> for RuntimeDiagnosticLocation {
    fn from(location: &DiagnosticLocation) -> Self {
        Self {
            source: location.source.clone(),
            start: location.start,
            end: location.end,
        }
    }
}

impl SourceCode for RuntimeDiagnosticLocation {
    fn read_span<'a>(
        &'a self,
        span: &SourceSpan,
        _context_lines_before: usize,
        _context_lines_after: usize,
    ) -> Result<Box<dyn SpanContents<'a> + 'a>, MietteError> {
        Ok(Box::new(MietteSpanContents::new_named(
            self.source.clone(),
            &[],
            *span,
            self.start.line - 1,
            self.start.column - 1,
            self.end.line - self.start.line + 1,
        )))
    }
}

fn project_location(diagnostic: &dyn Diagnostic) -> Option<DiagnosticLocation> {
    let source_code = diagnostic.source_code()?;
    let mut primary_labels = diagnostic.labels()?.filter(|label| label.primary());
    let label = primary_labels.next()?;
    if primary_labels.next().is_some() {
        return None;
    }
    let contents = source_code.read_span(label.inner(), 0, 0).ok()?;
    let source = contents.name()?.trim();
    if source.is_empty() {
        return None;
    }
    let start = DiagnosticPosition {
        line: contents.line() + 1,
        column: contents.column() + 1,
    };
    Some(DiagnosticLocation {
        source: source.to_owned(),
        start,
        end: advance_position(start, contents.data()),
    })
}

fn advance_position(mut position: DiagnosticPosition, bytes: &[u8]) -> DiagnosticPosition {
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' => {
                if bytes.get(index + 1) == Some(&b'\n') {
                    index += 1;
                }
                position.line += 1;
                position.column = 1;
            }
            b'\n' => {
                position.line += 1;
                position.column = 1;
            }
            _ => position.column += 1,
        }
        index += 1;
    }
    position
}

pub(super) fn publish<P: Serialize>(prepared: PreparedResponse<P>) -> ExitCode {
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    publish_to(prepared, &mut stdout.lock(), &mut stderr.lock())
}

fn publish_to<P: Serialize>(
    mut prepared: PreparedResponse<P>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    let mut frame = match serde_json::to_vec(&prepared.response) {
        Ok(frame) => frame,
        Err(error) => {
            report_publisher_failure(stderr, "serialize KAT Response", &error);
            return ExitCode::FAILURE;
        }
    };
    frame.push(b'\n');

    if let Some(rendered) = &prepared.rendered_diagnostic {
        let _ = stderr.write_all(rendered.0.as_bytes());
        let _ = stderr.write_all(b"\n");
        let _ = stderr.flush();
    }
    if let Err(error) = stdout.write_all(&frame).and_then(|()| stdout.flush()) {
        report_publisher_failure(stderr, "write KAT Response", &error);
        return ExitCode::FAILURE;
    }
    if let Some(pending_file) = prepared.pending_file.take() {
        pending_file.retain();
    }
    drop(prepared.session_lease.take());
    prepared.exit_code
}

fn report_publisher_failure(stderr: &mut dyn Write, action: &str, error: &dyn std::fmt::Display) {
    let _ = writeln!(stderr, "failed to {action}: {error}");
    let _ = stderr.flush();
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File, OpenOptions},
        io,
        path::Path,
    };

    use miette::{Diagnostic, NamedSource, SourceSpan, miette};
    use serde::Serializer;
    use thiserror::Error;

    use super::*;

    struct FailingWriter;

    struct FlushFailingWriter;

    struct SerializationFailure;

    struct LeaseFlushProbe {
        file: File,
        observed_shared_lease: bool,
    }

    impl Serialize for SerializationFailure {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            Err(serde::ser::Error::custom("cannot serialize"))
        }
    }

    #[derive(Debug, Error, Diagnostic)]
    #[error("invalid manifest")]
    #[diagnostic(help("repair the manifest"))]
    struct LocatedError {
        #[source_code]
        source_code: NamedSource<String>,
        #[label(primary, "invalid value")]
        span: SourceSpan,
        #[source]
        cause: LocatedCause,
    }

    #[derive(Debug, Error)]
    #[error("value is not valid")]
    struct LocatedCause;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed output"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Write for FlushFailingWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed output"))
        }
    }

    impl Write for LeaseFlushProbe {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            match self.file.try_lock() {
                Ok(()) => {
                    self.file.unlock()?;
                    Err(io::Error::other(
                        "exclusive lock succeeded before Response flush completed",
                    ))
                }
                Err(error) => {
                    let error = io::Error::from(error);
                    if error.kind() != io::ErrorKind::WouldBlock {
                        return Err(error);
                    }
                    self.observed_shared_lease = true;
                    Ok(())
                }
            }
        }
    }

    fn pending_success<P>(path: &Path, result: P) -> PreparedResponse<P> {
        fs::write(path, b"query result").unwrap();
        prepare_success_with_log_and_file(
            result,
            Some("query.log".to_owned()),
            PendingResponseFile::new(path.to_path_buf()),
        )
    }

    #[test]
    fn pending_query_result_is_retained_only_after_response_flush() {
        let temporary = tempfile::tempdir().unwrap();

        let serialization = temporary.path().join("serialization.ndjson");
        let prepared = pending_success(&serialization, SerializationFailure);
        assert_eq!(
            publish_to(prepared, &mut Vec::new(), &mut Vec::new()),
            ExitCode::FAILURE
        );
        assert!(!serialization.exists());

        let write = temporary.path().join("write.ndjson");
        let prepared = pending_success(&write, vec!["value"]);
        assert_eq!(
            publish_to(prepared, &mut FailingWriter, &mut Vec::new()),
            ExitCode::FAILURE
        );
        assert!(!write.exists());

        let flush = temporary.path().join("flush.ndjson");
        let prepared = pending_success(&flush, vec!["value"]);
        assert_eq!(
            publish_to(prepared, &mut FlushFailingWriter, &mut Vec::new()),
            ExitCode::FAILURE
        );
        assert!(!flush.exists());

        let success = temporary.path().join("success.ndjson");
        let prepared = pending_success(&success, vec!["value"]);
        assert_eq!(
            publish_to(prepared, &mut Vec::new(), &mut Vec::new()),
            ExitCode::SUCCESS
        );
        assert!(success.is_file());
    }

    #[test]
    fn session_lease_is_retained_through_response_flush() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("session.lock");
        let shared = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let lease = SessionLease::try_shared(shared).unwrap();
        let mut stdout = LeaseFlushProbe {
            file: OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .unwrap(),
            observed_shared_lease: false,
        };
        let prepared = retain_session_lease(prepare_success(vec!["value"]), lease);

        assert_eq!(
            publish_to(prepared, &mut stdout, &mut Vec::new()),
            ExitCode::SUCCESS
        );
        assert!(stdout.observed_shared_lease);
        stdout.file.try_lock().unwrap();
        stdout.file.unlock().unwrap();
    }

    #[test]
    fn stdout_failure_forces_failure_without_a_second_json_frame() {
        let prepared = prepare_success(vec!["value"]);
        let mut stderr = Vec::new();

        let exit = publish_to(prepared, &mut FailingWriter, &mut stderr);

        assert_eq!(exit, ExitCode::FAILURE);
        assert!(
            String::from_utf8(stderr)
                .unwrap()
                .contains("write KAT Response")
        );
    }

    #[test]
    fn serialization_failure_keeps_stdout_empty() {
        let prepared = prepare_success(SerializationFailure);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = publish_to(prepared, &mut stdout, &mut stderr);

        assert_eq!(exit, ExitCode::FAILURE);
        assert!(stdout.is_empty());
        assert!(
            String::from_utf8(stderr)
                .unwrap()
                .contains("serialize KAT Response")
        );
    }

    #[test]
    fn failure_json_and_terminal_projection_share_one_report() {
        let report = miette!(help = "repair the input", "operation failed");
        let prepared: PreparedResponse<Vec<String>> = prepare_cli_failure(report);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = publish_to(prepared, &mut stdout, &mut stderr);

        assert_eq!(exit, ExitCode::FAILURE);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "{\"status\":\"failure\",\"error\":{\"message\":\"operation failed\",\"help\":\"repair the input\"}}\n"
        );
        assert!(
            String::from_utf8(stderr)
                .unwrap()
                .contains("operation failed")
        );
    }

    #[test]
    fn operation_log_path_is_a_top_level_success_or_failure_field() {
        for (prepared, expected_status) in [
            (
                prepare_success_with_log(vec!["value"], Some("log.txt".to_owned())),
                "success",
            ),
            (
                prepare_cli_failure_with_log(
                    miette!("operation failed"),
                    Some("log.txt".to_owned()),
                ),
                "failure",
            ),
        ] {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();

            publish_to(prepared, &mut stdout, &mut stderr);

            let response: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
            assert_eq!(response["status"], expected_status);
            assert_eq!(response["log_path"], "log.txt");
        }
    }

    #[test]
    fn reliable_primary_label_projects_an_end_exclusive_location() {
        let report = miette::Report::new(LocatedError {
            source_code: NamedSource::new("pack.toml", "first\nbad\n".to_owned()),
            span: (6, 3).into(),
            cause: LocatedCause,
        });
        let prepared: PreparedResponse<Vec<String>> = prepare_cli_failure(report);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = publish_to(prepared, &mut stdout, &mut stderr);

        assert_eq!(exit, ExitCode::FAILURE);
        let response: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(
            response["error"]["location"],
            serde_json::json!({
                "source": "pack.toml",
                "start": {"line": 2, "column": 1},
                "end": {"line": 2, "column": 4}
            })
        );
        assert_eq!(response["error"]["message"], "invalid manifest");
        assert_eq!(
            response["error"]["causes"],
            serde_json::json!(["value is not valid"])
        );
        assert_eq!(response["error"]["help"], "repair the manifest");
        let rendered = String::from_utf8(stderr).unwrap();
        for evidence in [
            "invalid manifest",
            "value is not valid",
            "repair the manifest",
            "pack.toml",
            "bad",
        ] {
            assert!(
                rendered.contains(evidence),
                "terminal diagnostic omitted {evidence:?}: {rendered}"
            );
        }
    }

    #[test]
    fn runtime_diagnostic_terminal_projection_is_plain_but_json_is_unchanged() {
        let cause = "\x1b[31mred\x1b[0m\rline\0".to_owned();
        let prepared: PreparedResponse<Vec<String>> = prepare_runtime_failure(
            KatDiagnostic {
                message: "Runtime failure".to_owned(),
                causes: vec![cause.clone()],
                help: Some("repair Runtime input".to_owned()),
                location: Some(DiagnosticLocation {
                    source: "workflows/cpu.py".to_owned(),
                    start: DiagnosticPosition { line: 3, column: 5 },
                    end: DiagnosticPosition { line: 3, column: 8 },
                }),
            },
            "log.txt".to_owned(),
        );
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        publish_to(prepared, &mut stdout, &mut stderr);

        let response: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(response["error"]["causes"], serde_json::json!([cause]));
        let terminal = String::from_utf8(stderr).unwrap();
        assert!(!terminal.contains('\x1b'));
        assert!(!terminal.contains('\0'));
        assert!(terminal.contains("red\nline\\u{0000}"));
        assert!(terminal.contains("repair Runtime input"));
        assert!(terminal.contains("workflows/cpu.py:3:5"));
    }

    #[test]
    fn runtime_diagnostic_rejects_explicit_empty_causes() {
        let error = serde_json::from_value::<KatDiagnostic>(serde_json::json!({
            "message": "Runtime failure",
            "causes": []
        }))
        .expect_err("an explicit empty causes field is not a valid sparse Diagnostic");

        assert!(
            error
                .to_string()
                .contains("causes must be omitted or non-empty")
        );
    }

    #[test]
    fn runtime_diagnostic_rejects_non_relative_location_sources() {
        for source in [
            "/tmp/request.json",
            "../request.json",
            "workflows/../request.json",
            "workflows\\cpu.py",
            "C:/private/request.json",
            "workflows//cpu.py",
            "workflows/cpu.py\nforged",
        ] {
            let diagnostic = KatDiagnostic {
                message: "Runtime failure".to_owned(),
                causes: Vec::new(),
                help: None,
                location: Some(DiagnosticLocation {
                    source: source.to_owned(),
                    start: DiagnosticPosition { line: 1, column: 1 },
                    end: DiagnosticPosition { line: 1, column: 2 },
                }),
            };

            assert!(!diagnostic.validate(), "accepted private source {source:?}");
        }
        assert!(valid_runtime_location_source("workflows/分析.py"));
    }
}
