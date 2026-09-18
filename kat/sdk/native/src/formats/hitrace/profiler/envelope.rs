use crate::proto::ProfilerPluginData;

const CONFIG_SUFFIX: &str = "_config";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginEnvelopeKind {
    Data,
    Config,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PluginEnvelope<'a> {
    pub(crate) envelope_name: &'a str,
    pub(crate) kind: PluginEnvelopeKind,
    pub(crate) payload: &'a [u8],
    pub(crate) status: u32,
    pub(crate) clock_id: i32,
    pub(crate) tv_sec: u64,
    pub(crate) tv_nsec: u64,
    pub(crate) version: &'a str,
    pub(crate) sample_interval: u32,
    pub(crate) section_start: usize,
}

impl<'a> PluginEnvelope<'a> {
    pub(crate) fn from_profiler_plugin_data(
        message: &'a ProfilerPluginData,
        section_start: usize,
    ) -> Self {
        let kind = if message.name.ends_with(CONFIG_SUFFIX) {
            PluginEnvelopeKind::Config
        } else {
            PluginEnvelopeKind::Data
        };

        Self {
            envelope_name: message.name.as_str(),
            kind,
            payload: message.data.as_slice(),
            status: message.status,
            clock_id: message.clock_id,
            tv_sec: message.tv_sec,
            tv_nsec: message.tv_nsec,
            version: message.version.as_str(),
            sample_interval: message.sample_interval,
            section_start,
        }
    }
}
