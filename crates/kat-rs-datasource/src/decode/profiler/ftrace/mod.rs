//! ftrace plugin domain decoding.

use crate::{
    decode::profiler::{ProfilerPayloadRoute, ProfilerPluginRoute, emit_typed_payload},
    proto::TracePluginResult,
};

const FTRACE_PLUGIN_NAME: &str = "ftrace-plugin";

pub(super) const FTRACE_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: FTRACE_PLUGIN_NAME,
    config: None,
    data: ProfilerPayloadRoute {
        root_message: "TracePluginResult",
        emit: emit_typed_payload::<TracePluginResult>,
    },
};
