use std::{
    io::{self, Read},
    process::{Child, ExitStatus},
    sync::mpsc::{self, RecvTimeoutError, SyncSender},
    thread,
    time::{Duration, Instant},
};

use crate::{
    operation_log::{OperationLog, OperationLogError},
    text_projection::TextProjection,
};

use super::{ExchangeError, RuntimeInfrastructureError};

const HOST_EXIT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const HOST_EXIT_STALL_LIMIT: Duration = Duration::from_secs(2);
const STREAM_EVENT_BUFFER_CAPACITY: usize = 32;

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

pub(super) trait RuntimeLogSink {
    fn append(&mut self, bytes: &[u8]) -> Result<(), OperationLogError>;
}

impl RuntimeLogSink for OperationLog {
    fn append(&mut self, bytes: &[u8]) -> Result<(), OperationLogError> {
        OperationLog::append(self, bytes)
    }
}

pub(super) fn capture_streams(
    child: &mut Child,
    log: &mut impl RuntimeLogSink,
) -> Result<ExitStatus, ExchangeError> {
    let stdout = child
        .stdout
        .take()
        .ok_or(RuntimeInfrastructureError::MissingPipe("stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or(RuntimeInfrastructureError::MissingPipe("stderr"))?;
    let (sender, receiver) = mpsc::sync_channel(STREAM_EVENT_BUFFER_CAPACITY);
    let stdout_thread = spawn_stream_reader(stdout, Stream::Stdout, sender.clone());
    let stderr_thread = spawn_stream_reader(stderr, Stream::Stderr, sender);
    let mut stdout_projection = TextProjection::new("stdout");
    let mut stderr_projection = TextProjection::new("stderr");
    let mut stdout_finished = false;
    let mut stderr_finished = false;
    let mut status = None;
    let mut drain_stall_deadline = None;
    loop {
        if stdout_finished && stderr_finished {
            stdout_thread
                .join()
                .map_err(|_| RuntimeInfrastructureError::StreamThread("stdout"))?;
            stderr_thread
                .join()
                .map_err(|_| RuntimeInfrastructureError::StreamThread("stderr"))?;
            return match status {
                Some(status) => Ok(status),
                None => child
                    .wait()
                    .map_err(RuntimeInfrastructureError::WaitHost)
                    .map_err(Into::into),
            };
        }
        if status.is_none()
            && let Some(exit_status) = child
                .try_wait()
                .map_err(RuntimeInfrastructureError::WaitHost)?
        {
            status = Some(exit_status);
            drain_stall_deadline = Some(Instant::now() + HOST_EXIT_STALL_LIMIT);
        }
        if let Some(deadline) = drain_stall_deadline
            && Instant::now() >= deadline
        {
            if !stdout_finished {
                log.append(stdout_projection.finish().as_bytes())
                    .map_err(ExchangeError::Log)?;
            }
            if !stderr_finished {
                log.append(stderr_projection.finish().as_bytes())
                    .map_err(ExchangeError::Log)?;
            }
            return Err(RuntimeInfrastructureError::StreamDrainTimeout.into());
        }
        let now = Instant::now();
        let timeout = drain_stall_deadline
            .map(|deadline| deadline.saturating_duration_since(now))
            .unwrap_or(HOST_EXIT_POLL_INTERVAL)
            .min(HOST_EXIT_POLL_INTERVAL);
        let event = match receiver.recv_timeout(timeout) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return Err(RuntimeInfrastructureError::StreamChannel.into());
            }
        };
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
                if status.is_some() {
                    drain_stall_deadline = Some(Instant::now() + HOST_EXIT_STALL_LIMIT);
                }
            }
            StreamEvent::Error(stream, source) => {
                let error = RuntimeInfrastructureError::ReadStream {
                    stream: stream.name(),
                    source,
                };
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
            StreamEvent::Finished(stream) => {
                let projection = match stream {
                    Stream::Stdout => {
                        stdout_finished = true;
                        &mut stdout_projection
                    }
                    Stream::Stderr => {
                        stderr_finished = true;
                        &mut stderr_projection
                    }
                };
                let text = projection.finish();
                if let Err(error) = log.append(text.as_bytes()) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ExchangeError::Log(error));
                }
                if status.is_some() {
                    drain_stall_deadline = Some(Instant::now() + HOST_EXIT_STALL_LIMIT);
                }
            }
        }
    }
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
    sender: SyncSender<StreamEvent>,
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
