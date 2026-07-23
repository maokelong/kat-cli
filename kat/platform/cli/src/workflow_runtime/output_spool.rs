use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::Path,
    process::Stdio,
};

use crate::{
    operation_log::{OperationLog, OperationLogError},
    text_projection::TextProjection,
};

use super::{ExchangeError, RuntimeInfrastructureError};

pub(super) struct RuntimeOutputSpool {
    stdout: File,
    stderr: File,
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
            stdout: create("stdout.log")?,
            stderr: create("stderr.log")?,
        })
    }

    pub(super) fn stdio(&self) -> Result<(Stdio, Stdio), RuntimeInfrastructureError> {
        Ok((
            self.clone_for("stdout", &self.stdout)?,
            self.clone_for("stderr", &self.stderr)?,
        ))
    }

    pub(super) fn append_to(mut self, log: &mut OperationLog) -> Result<(), ExchangeError> {
        append_stream(&mut self.stdout, "stdout", log)?;
        append_stream(&mut self.stderr, "stderr", log)
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
) -> Result<(), ExchangeError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|source| RuntimeInfrastructureError::ReadOutputSpool { stream, source })?;
    let mut stripper = strip_ansi::StripStream::new();
    let mut projection = TextProjection::new(stream);
    let mut buffer = [0_u8; 8192];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|source| RuntimeInfrastructureError::ReadOutputSpool { stream, source })?;
        if count == 0 {
            break;
        }
        let mut plain = Vec::with_capacity(count);
        stripper.push(&buffer[..count], &mut plain);
        log.append(projection.push(&plain).as_bytes())
            .map_err(ExchangeError::Log)?;
    }
    stripper.finish();
    log.append(projection.finish().as_bytes())
        .map_err(ExchangeError::Log)
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

        append_stream(&mut file, "stdout", &mut log).unwrap();

        let output = String::from_utf8(log.0).unwrap();
        assert!(output.starts_with(
            "first\n\u{1F600}\\u{0000}[KAT: invalid UTF-8 in Runtime stdout was replaced]\n\u{FFFD}\n"
        ));
        assert_eq!(
            output.bytes().filter(|byte| *byte == b'x').count(),
            512 * 1024
        );
        assert!(!output.contains('\x1b'));
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
            append_stream(&mut file, "stderr", &mut FailingLog),
            Err(ExchangeError::Log(_))
        ));
    }
}
