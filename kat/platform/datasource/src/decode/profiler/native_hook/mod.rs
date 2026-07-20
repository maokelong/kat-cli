//! native hook plugin domain decoding.

use crate::{
    decode::profiler::{ProfilerPayloadRoute, ProfilerPluginRoute, emit_typed_payload},
    proto::{BatchNativeHookData, NativeHookConfig},
};

const NATIVE_HOOK_PLUGIN_NAME: &str = "nativehook";
const HOOK_DAEMON_PLUGIN_NAME: &str = "hookdaemon";

pub(super) const NATIVE_HOOK_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: NATIVE_HOOK_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: "NativeHookConfig",
        emit: emit_typed_payload::<NativeHookConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: "BatchNativeHookData",
        emit: emit_typed_payload::<BatchNativeHookData>,
    },
};

pub(super) const HOOK_DAEMON_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: HOOK_DAEMON_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: "NativeHookConfig",
        emit: emit_typed_payload::<NativeHookConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: "BatchNativeHookData",
        emit: emit_typed_payload::<BatchNativeHookData>,
    },
};
