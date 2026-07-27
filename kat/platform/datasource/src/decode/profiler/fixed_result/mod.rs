// fixed result profiler plugin decoding.

use crate::{
    decode::profiler::{ProfilerPayloadRoute, ProfilerPluginRoute, emit_typed_payload},
    proto::kat::{
        cpu_data::{CpuConfig, CpuData},
        diskio_data::{DiskioConfig, DiskioData},
        gpu_data::{GpuConfig, GpuData},
        memory_data::{MemoryConfig, MemoryData},
        network_data::{NetworkConfig, NetworkDatas},
        process_data::{ProcessConfig, ProcessData},
    },
};

const CPU_PLUGIN_NAME: &str = "cpu-plugin";
const MEMORY_PLUGIN_NAME: &str = "memory-plugin";
const PROCESS_PLUGIN_NAME: &str = "process-plugin";
const DISKIO_PLUGIN_NAME: &str = "diskio-plugin";
const NETWORK_PLUGIN_NAME: &str = "network-plugin";
const GPU_PLUGIN_NAME: &str = "gpu-plugin";

pub(super) const CPU_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: CPU_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: "CpuConfig",
        emit: emit_typed_payload::<CpuConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: "CpuData",
        emit: emit_typed_payload::<CpuData>,
    },
};
pub(super) const MEMORY_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: MEMORY_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: "MemoryConfig",
        emit: emit_typed_payload::<MemoryConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: "MemoryData",
        emit: emit_typed_payload::<MemoryData>,
    },
};
pub(super) const PROCESS_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: PROCESS_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: "ProcessConfig",
        emit: emit_typed_payload::<ProcessConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: "ProcessData",
        emit: emit_typed_payload::<ProcessData>,
    },
};
pub(super) const DISKIO_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: DISKIO_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: "DiskioConfig",
        emit: emit_typed_payload::<DiskioConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: "DiskioData",
        emit: emit_typed_payload::<DiskioData>,
    },
};
pub(super) const NETWORK_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: NETWORK_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: "NetworkConfig",
        emit: emit_typed_payload::<NetworkConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: "NetworkDatas",
        emit: emit_typed_payload::<NetworkDatas>,
    },
};
pub(super) const GPU_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: GPU_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: "GpuConfig",
        emit: emit_typed_payload::<GpuConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: "GpuData",
        emit: emit_typed_payload::<GpuData>,
    },
};
