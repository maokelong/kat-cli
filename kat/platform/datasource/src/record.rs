// Pre-sink trace record stream shared by format/domain decoders and sinks.

use crate::payload_value::{PayloadValue, to_payload_value};
use anyhow::{Context, Result};
use serde::Serialize;

#[derive(Clone, Debug)]
pub(crate) struct DecodedPayload {
    pub(crate) plugin_name: String,
    pub(crate) root_message: String,
    pub(crate) message: PayloadValue,
}

impl DecodedPayload {
    pub(crate) fn from_typed_message<T>(
        plugin_name: impl Into<String>,
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
            plugin_name: plugin_name.into(),
            root_message: root_message.clone(),
            message,
        })
    }
}

pub(crate) enum TraceRecord {
    DecodedPayload(Box<DecodedPayload>),
}

pub(crate) trait TraceRecordSink {
    fn push(&mut self, record: TraceRecord) -> Result<()>;
}
