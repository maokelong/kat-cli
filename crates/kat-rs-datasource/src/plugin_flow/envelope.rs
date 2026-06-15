// envelope 抽出插件名、配置/数据类型和载荷元信息，作为 registry 分发边界。
use crate::proto::ProfilerPluginData;

const CONFIG_SUFFIX: &str = "_config";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginEnvelopeKind {
    Data,
    Config,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PluginEnvelope<'a> {
    pub(crate) plugin_name: &'a str,
    pub(crate) envelope_name: &'a str,
    pub(crate) kind: PluginEnvelopeKind,
    pub(crate) payload: &'a [u8],
    pub(crate) version: &'a str,
    pub(crate) sample_interval: u32,
    pub(crate) section_start: usize,
}

impl<'a> PluginEnvelope<'a> {
    pub(crate) fn from_profiler_plugin_data(
        message: &'a ProfilerPluginData,
        section_start: usize,
    ) -> Self {
        let (plugin_name, kind) =
            if let Some(plugin_name) = message.name.strip_suffix(CONFIG_SUFFIX) {
                (plugin_name, PluginEnvelopeKind::Config)
            } else {
                (message.name.as_str(), PluginEnvelopeKind::Data)
            };

        Self {
            plugin_name,
            envelope_name: message.name.as_str(),
            kind,
            payload: message.data.as_slice(),
            version: message.version.as_str(),
            sample_interval: message.sample_interval,
            section_start,
        }
    }
}
