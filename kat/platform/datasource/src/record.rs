// Pre-sink trace record stream shared by format/domain decoders and sinks.

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{
    domains::{
        ftrace::{FtraceCaptureRecord, FtraceRecord},
        native_hook::NativeHookRecord,
    },
    payload_value::{PayloadValue, to_payload_value},
    proto::ProfilerPluginData,
};

#[derive(Clone, Debug)]
pub(crate) struct DecodedPayload {
    pub(crate) root_message: String,
    pub(crate) message: PayloadValue,
}

impl DecodedPayload {
    pub(crate) fn from_typed_message<T>(
        root_message: impl Into<String>,
        message: &T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        let root_message = root_message.into();
        let message = to_payload_value(message)
            .with_context(|| format!("failed to serialize decoded payload {root_message}"))?;

        Ok(Self {
            root_message,
            message,
        })
    }
}

pub(crate) enum TraceRecord {
    ProfilerPluginData(ProfilerPluginData),
    FtraceCapture(FtraceCaptureRecord),
    Ftrace(Box<FtraceRecord>),
    NativeHook(Box<NativeHookRecord>),
    DecodedPayload(Box<DecodedPayload>),
}

pub(crate) trait TraceRecordSink {
    fn accepts_decoded_payloads(&self) -> bool {
        false
    }

    fn accepts_source_records(&self) -> bool {
        true
    }

    fn push(&mut self, record: TraceRecord) -> Result<()>;
}
