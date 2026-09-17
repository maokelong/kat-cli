use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    process::Stdio,
};

use crate::{
    operation_log::{OperationLog, OperationLogError},
    text_projection::TextProjection,
};

use super::{ExchangeError, RuntimeInfrastructureError};

pub(super) struct RuntimeOutputSpool {
    stdout: Option<File>,
    stderr: File,
}

pub(super) struct RuntimeOutputMirror<'a> {
    log: &'a mut OperationLog,
    terminal: &'a mut dyn Write,
    projection: StreamProjection,
}

struct StreamProjection {
    stripper: strip_ansi::StripStream,
    projection: TextProjection,
    wrote_output: bool,
    ended_with_newline: bool,
}

impl RuntimeOutputSpool {
    pub(super) fn create(control: &Path) -> Result<Self, RuntimeInfrastructureError> {
        let create = |name| {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(control.join(name))
                .map_err(RuntimeInfrastructureError::CreateOutputSpool)
        };
        Ok(Self {
            stdout: Some(create("stdout.log")?),
            stderr: create("stderr.log")?,
        })
    }

    pub(super) fn create_stderr_only(control: &Path) -> Result<Self, RuntimeInfrastructureError> {
        let stderr = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(control.join("stderr.log"))
            .map_err(RuntimeInfrastructureError::CreateOutputSpool)?;
        Ok(Self {
            stdout: None,
            stderr,
        })
    }

    pub(super) fn stdio(&self) -> Result<(Stdio, Stdio), RuntimeInfrastructureError> {
        let stdout = self
            .stdout
            .as_ref()
            .expect("a complete Runtime output spool owns stdout");
        Ok((
            self.clone_for("stdout", stdout)?,
            self.clone_for("stderr", &self.stderr)?,
        ))
    }

    pub(super) fn stderr_stdio(&self) -> Result<Stdio, RuntimeInfrastructureError> {
        self.clone_for("stderr", &self.stderr)
    }

    pub(super) fn append_to(mut self, log: &mut OperationLog) -> Result<(), ExchangeError> {
        let mut terminal = None;
        let stdout = self
            .stdout
            .as_mut()
            .expect("a complete Runtime output spool owns stdout");
        append_stream(stdout, "stdout", log, &mut terminal)?;
        append_stream(&mut self.stderr, "stderr", log, &mut terminal)
    }

    pub(super) fn append_stderr_to(mut self, log: &mut OperationLog) -> Result<(), ExchangeError> {
        let mut terminal = None;
        append_stream(&mut self.stderr, "stderr", log, &mut terminal)
    }

    fn clone_for(
        &self,
        stream: &'static str,
        file: &File,
    ) -> Result<Stdio, RuntimeInfrastructureError> {
        file.try_clone()
            .map(Stdio::from)
            .map_err(|source| RuntimeInfrastructureError::CloneOutputSpool { stream, source })
    }
}

impl<'a> RuntimeOutputMirror<'a> {
    pub(super) fn new(log: &'a mut OperationLog, terminal: &'a mut dyn Write) -> Self {
        Self {
            log,
            terminal,
            projection: StreamProjection::new("output"),
        }
    }

    pub(super) fn append(&mut self, bytes: &[u8]) -> Result<(), ExchangeError> {
        let output = self.projection.push(bytes);
        let mut terminal = Some(&mut *self.terminal as &mut dyn Write);
        append_projection(&mut self.projection, &output, self.log, &mut terminal)
    }

    pub(super) fn finish(mut self) -> Result<(), ExchangeError> {
        let output = self.projection.finish();
        let mut terminal = Some(&mut *self.terminal as &mut dyn Write);
        append_projection(&mut self.projection, &output, self.log, &mut terminal)?;
        if self.projection.needs_line_boundary() {
            let mut terminal = Some(&mut *self.terminal as &mut dyn Write);
            append_projection(&mut self.projection, "\n", self.log, &mut terminal)?;
        }
        self.terminal
            .flush()
            .map_err(RuntimeInfrastructureError::MirrorRuntimeOutput)?;
        Ok(())
    }
}

impl StreamProjection {
    fn new(stream: &'static str) -> Self {
        Self {
            stripper: strip_ansi::StripStream::new(),
            projection: TextProjection::new(stream),
            wrote_output: false,
            ended_with_newline: false,
        }
    }

    fn push(&mut self, bytes: &[u8]) -> String {
        let mut plain = Vec::with_capacity(bytes.len());
        self.stripper.push(bytes, &mut plain);
        self.projection.push(&plain)
    }

    fn finish(&mut self) -> String {
        self.stripper.finish();
        self.projection.finish()
    }

    fn needs_line_boundary(&self) -> bool {
        self.wrote_output && !self.ended_with_newline
    }
}

trait RuntimeLogSink {
    fn append(&mut self, bytes: &[u8]) -> Result<(), OperationLogError>;
}

impl RuntimeLogSink for OperationLog {
    fn append(&mut self, bytes: &[u8]) -> Result<(), OperationLogError> {
        OperationLog::append(self, bytes)
    }
}

fn append_stream(
    file: &mut File,
    stream: &'static str,
    log: &mut impl RuntimeLogSink,
    terminal: &mut Option<&mut dyn Write>,
) -> Result<(), ExchangeError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|source| RuntimeInfrastructureError::ReadOutputSpool { stream, source })?;
    let mut projection = StreamProjection::new(stream);
    let mut buffer = [0_u8; 8192];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|source| RuntimeInfrastructureError::ReadOutputSpool { stream, source })?;
        if count == 0 {
            break;
        }
        let output = projection.push(&buffer[..count]);
        append_projection(&mut projection, &output, log, terminal)?;
    }
    let output = projection.finish();
    append_projection(&mut projection, &output, log, terminal)?;
    if projection.needs_line_boundary() {
        append_projection(&mut projection, "\n", log, terminal)?;
    }
    Ok(())
}

fn append_projection(
    projection: &mut StreamProjection,
    output: &str,
    log: &mut impl RuntimeLogSink,
    terminal: &mut Option<&mut dyn Write>,
) -> Result<(), ExchangeError> {
    if output.is_empty() {
        return Ok(());
    }
    projection.wrote_output = true;
    projection.ended_with_newline = output.ends_with('\n');
    log.append(output.as_bytes()).map_err(ExchangeError::Log)?;
    if let Some(terminal) = terminal.as_deref_mut() {
        terminal
            .write_all(output.as_bytes())
            .map_err(RuntimeInfrastructureError::MirrorRuntimeOutput)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    #[derive(Default)]
    struct RecordingLog(Vec<u8>);

    impl RuntimeLogSink for RecordingLog {
        fn append(&mut self, bytes: &[u8]) -> Result<(), OperationLogError> {
            self.0.extend_from_slice(bytes);
            Ok(())
        }
    }

    #[test]
    fn spool_projects_large_plain_output_without_loading_the_whole_stream() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"\x1b[31mfirst\r\n\xF0\x9F\x98\x80\0\xFF\n")
            .unwrap();
        file.write_all(&vec![b'x'; 512 * 1024]).unwrap();
        let mut log = RecordingLog::default();

        append_stream(&mut file, "stdout", &mut log, &mut None).unwrap();

        let output = String::from_utf8(log.0).unwrap();
        assert!(output.starts_with(
            "first\n\u{1F600}\\u{0000}[KAT: invalid UTF-8 in Runtime stdout was replaced]\n\u{FFFD}\n"
        ));
        assert_eq!(
            output.bytes().filter(|byte| *byte == b'x').count(),
            512 * 1024
        );
        assert!(!output.contains('\x1b'));
        assert!(output.ends_with('\n'));
    }

    #[test]
    fn spool_terminates_each_non_empty_stream_with_one_line_boundary() {
        let mut stdout = tempfile::tempfile().unwrap();
        stdout.write_all(b"stdout without newline").unwrap();
        let mut stderr = tempfile::tempfile().unwrap();
        stderr.write_all(b"stderr already terminated\n").unwrap();
        let mut log = RecordingLog::default();

        append_stream(&mut stdout, "stdout", &mut log, &mut None).unwrap();
        append_stream(&mut stderr, "stderr", &mut log, &mut None).unwrap();

        assert_eq!(
            String::from_utf8(log.0).unwrap(),
            "stdout without newline\nstderr already terminated\n"
        );
    }

    #[test]
    fn merged_diagnostics_use_one_projection_and_one_final_line_boundary() {
        let mut projection = StreamProjection::new("output");
        let mut output = projection.push(b"stdout without newline");
        output.push_str(&projection.push(b"stderr already terminated\n"));
        output.push_str(&projection.finish());
        if projection.needs_line_boundary() {
            output.push('\n');
        }

        assert_eq!(output, "stdout without newlinestderr already terminated\n");
    }

    #[test]
    fn spool_mirrors_the_same_normalized_output_to_the_terminal() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"\x1b[31mpytest\x1b[0m\r\n").unwrap();
        let mut log = RecordingLog::default();
        let mut terminal = Vec::new();

        append_stream(&mut file, "stderr", &mut log, &mut Some(&mut terminal)).unwrap();

        assert_eq!(terminal, log.0);
        assert_eq!(String::from_utf8(terminal).unwrap(), "pytest\n");
    }

    #[test]
    fn empty_stream_does_not_add_a_log_line() {
        let mut file = tempfile::tempfile().unwrap();
        let mut log = RecordingLog::default();

        append_stream(&mut file, "stdout", &mut log, &mut None).unwrap();

        assert!(log.0.is_empty());
    }

    #[test]
    fn stderr_only_spool_leaves_stdout_available_for_control_frames() {
        let temporary = tempfile::tempdir().unwrap();
        let mut spool = RuntimeOutputSpool::create_stderr_only(temporary.path()).unwrap();

        assert!(!temporary.path().join("stdout.log").exists());
        spool.stderr.write_all(b"PACK output\n").unwrap();
        let mut log = RecordingLog::default();
        let mut terminal = None;
        append_stream(&mut spool.stderr, "stderr", &mut log, &mut terminal).unwrap();

        assert_eq!(log.0, b"PACK output\n");
    }

    #[test]
    fn spool_propagates_log_failures() {
        struct FailingLog;
        impl RuntimeLogSink for FailingLog {
            fn append(&mut self, _bytes: &[u8]) -> Result<(), OperationLogError> {
                Err(OperationLogError::Write {
                    path: PathBuf::from("partial.log"),
                    source: std::io::Error::other("injected log failure"),
                })
            }
        }

        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"output").unwrap();
        assert!(matches!(
            append_stream(&mut file, "stderr", &mut FailingLog, &mut None),
            Err(ExchangeError::Log(_))
        ));
    }
}
