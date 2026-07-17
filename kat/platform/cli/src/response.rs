use std::{io::Write, process::ExitCode};

use miette::Diagnostic;
use serde::{Deserialize, Serialize};

use crate::text_projection::project_complete_text;

pub(super) struct PreparedResponse<P> {
    response: KatResponse<P>,
    rendered_diagnostic: Option<RenderedDiagnostic>,
    exit_code: ExitCode,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum KatResponse<P> {
    Success {
        result: P,
        #[serde(skip_serializing_if = "Option::is_none")]
        log_path: Option<String>,
    },
    Failure {
        error: KatDiagnostic,
        #[serde(skip_serializing_if = "Option::is_none")]
        log_path: Option<String>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct KatDiagnostic {
    message: String,
    #[serde(default)]
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

impl KatDiagnostic {
    pub(super) fn validate(&self) -> bool {
        !self.message.trim().is_empty()
            && self.causes.iter().all(|cause| !cause.trim().is_empty())
            && self
                .help
                .as_ref()
                .is_none_or(|help| !help.trim().is_empty())
            && self.location.as_ref().is_none_or(DiagnosticLocation::valid)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DiagnosticLocation {
    source: String,
    start: DiagnosticPosition,
    end: DiagnosticPosition,
}

impl DiagnosticLocation {
    fn valid(&self) -> bool {
        !self.source.trim().is_empty()
            && self.start.line > 0
            && self.start.column > 0
            && self.end.line > 0
            && self.end.column > 0
            && (self.end.line, self.end.column) >= (self.start.line, self.start.column)
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
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
    PreparedResponse {
        response: KatResponse::Success { result, log_path },
        rendered_diagnostic: None,
        exit_code: ExitCode::SUCCESS,
    }
}

pub(super) fn success_response_size<P: Serialize>(
    result: &P,
    log_path: Option<&str>,
) -> Result<usize, serde_json::Error> {
    serde_json::to_vec(&KatResponse::Success {
        result,
        log_path: log_path.map(str::to_owned),
    })
    .map(|frame| frame.len())
}

pub(super) fn prepare_cli_failure<P>(report: miette::Report) -> PreparedResponse<P> {
    prepare_cli_failure_with_log(report, None)
}

pub(super) fn prepare_cli_failure_with_log<P>(
    report: miette::Report,
    log_path: Option<String>,
) -> PreparedResponse<P> {
    let diagnostic: &dyn Diagnostic = report.as_ref();
    let message = diagnostic.to_string();
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
    let rendered_diagnostic = RenderedDiagnostic(project_complete_text(&format!("{report:?}")));
    let location = project_location(diagnostic);

    PreparedResponse {
        response: KatResponse::Failure {
            error: KatDiagnostic {
                message,
                causes,
                help,
                location,
            },
            log_path,
        },
        rendered_diagnostic: Some(rendered_diagnostic),
        exit_code: ExitCode::FAILURE,
    }
}

pub(super) fn prepare_runtime_failure<P>(
    diagnostic: KatDiagnostic,
    log_path: String,
) -> PreparedResponse<P> {
    let rendered_diagnostic = RenderedDiagnostic(render_runtime_diagnostic(&diagnostic));
    PreparedResponse {
        response: KatResponse::Failure {
            error: diagnostic,
            log_path: Some(log_path),
        },
        rendered_diagnostic: Some(rendered_diagnostic),
        exit_code: ExitCode::FAILURE,
    }
}

fn render_runtime_diagnostic(diagnostic: &KatDiagnostic) -> String {
    let mut rendered = diagnostic.message.clone();
    for cause in &diagnostic.causes {
        rendered.push_str("\n  caused by: ");
        rendered.push_str(cause);
    }
    if let Some(help) = &diagnostic.help {
        rendered.push_str("\n  help: ");
        rendered.push_str(help);
    }
    if let Some(location) = &diagnostic.location {
        rendered.push_str(&format!(
            "\n  at {}:{}:{}",
            location.source, location.start.line, location.start.column
        ));
    }
    project_complete_text(&rendered)
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
    prepared: PreparedResponse<P>,
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

    if let Some(rendered) = prepared.rendered_diagnostic {
        let _ = stderr.write_all(rendered.0.as_bytes());
        let _ = stderr.write_all(b"\n");
        let _ = stderr.flush();
    }
    if let Err(error) = stdout.write_all(&frame).and_then(|()| stdout.flush()) {
        report_publisher_failure(stderr, "write KAT Response", &error);
        return ExitCode::FAILURE;
    }
    prepared.exit_code
}

fn report_publisher_failure(stderr: &mut dyn Write, action: &str, error: &dyn std::fmt::Display) {
    let _ = writeln!(stderr, "failed to {action}: {error}");
    let _ = stderr.flush();
}

#[cfg(test)]
mod tests {
    use std::io;

    use miette::{Diagnostic, NamedSource, SourceSpan, miette};
    use serde::Serializer;
    use thiserror::Error;

    use super::*;

    struct FailingWriter;

    struct SerializationFailure;

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
                help: None,
                location: None,
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
    }
}
