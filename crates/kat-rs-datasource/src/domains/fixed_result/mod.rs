// fixed result profiler plugin domain decoding.

use serde::{Deserialize, Serialize};

use crate::formats::hitrace::profiler::{PluginEnvelope, PluginEnvelopeKind};

mod records {
    include!(concat!(env!("OUT_DIR"), "/fixed_result_records.rs"));
}

pub(crate) use records::{FIXED_RESULT_PLUGIN_DECODERS, FixedResultRecord};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ProfilerEnvelopeMeta {
    envelope_plugin_name: String,
    envelope_name: String,
    envelope_kind: String,
    envelope_version: String,
    envelope_sample_interval: u32,
    envelope_clock_id: i32,
    envelope_tv_sec: u64,
    envelope_tv_nsec: u64,
    envelope_section_start: u64,
}

impl ProfilerEnvelopeMeta {
    pub(crate) fn from_envelope(envelope: &PluginEnvelope<'_>) -> Self {
        Self {
            envelope_plugin_name: envelope.plugin_name.to_string(),
            envelope_name: envelope.envelope_name.to_string(),
            envelope_kind: match envelope.kind {
                PluginEnvelopeKind::Config => "config",
                PluginEnvelopeKind::Data => "data",
            }
            .to_string(),
            envelope_version: envelope.version.to_string(),
            envelope_sample_interval: envelope.sample_interval,
            envelope_clock_id: envelope.clock_id,
            envelope_tv_sec: envelope.tv_sec,
            envelope_tv_nsec: envelope.tv_nsec,
            envelope_section_start: envelope.section_start as u64,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FixedResultMessage<T> {
    pub(crate) meta: ProfilerEnvelopeMeta,
    pub(crate) message: T,
}

impl<T> FixedResultMessage<T> {
    pub(crate) fn new(meta: ProfilerEnvelopeMeta, message: T) -> Self {
        Self { meta, message }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct FixedResultChildMeta {
    envelope_plugin_name: String,
    envelope_name: String,
    envelope_kind: String,
    envelope_version: String,
    envelope_sample_interval: u32,
    envelope_clock_id: i32,
    envelope_tv_sec: u64,
    envelope_tv_nsec: u64,
    envelope_section_start: u64,
    child_index: u32,
}

impl FixedResultChildMeta {
    pub(crate) fn new(envelope: ProfilerEnvelopeMeta, child_index: u32) -> Self {
        Self {
            envelope_plugin_name: envelope.envelope_plugin_name,
            envelope_name: envelope.envelope_name,
            envelope_kind: envelope.envelope_kind,
            envelope_version: envelope.envelope_version,
            envelope_sample_interval: envelope.envelope_sample_interval,
            envelope_clock_id: envelope.envelope_clock_id,
            envelope_tv_sec: envelope.envelope_tv_sec,
            envelope_tv_nsec: envelope.envelope_tv_nsec,
            envelope_section_start: envelope.envelope_section_start,
            child_index,
        }
    }
}
