// fixed result profiler plugin decoding.

use crate::{
    decode::profiler::{
        ProfilerPayloadRoute, ProfilerPluginRoute, emit_typed_payload,
        roots::{
            CPU_CONFIG_ROOT_MESSAGE, CPU_DATA_ROOT_MESSAGE, DISKIO_CONFIG_ROOT_MESSAGE,
            DISKIO_DATA_ROOT_MESSAGE, GPU_CONFIG_ROOT_MESSAGE, GPU_DATA_ROOT_MESSAGE,
            MEMORY_CONFIG_ROOT_MESSAGE, MEMORY_DATA_ROOT_MESSAGE, NETWORK_CONFIG_ROOT_MESSAGE,
            NETWORK_DATA_ROOT_MESSAGE, PROCESS_CONFIG_ROOT_MESSAGE, PROCESS_DATA_ROOT_MESSAGE,
        },
    },
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
        root_message: CPU_CONFIG_ROOT_MESSAGE,
        emit: emit_typed_payload::<CpuConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: CPU_DATA_ROOT_MESSAGE,
        emit: emit_typed_payload::<CpuData>,
    },
};
pub(super) const MEMORY_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: MEMORY_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: MEMORY_CONFIG_ROOT_MESSAGE,
        emit: emit_typed_payload::<MemoryConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: MEMORY_DATA_ROOT_MESSAGE,
        emit: emit_typed_payload::<MemoryData>,
    },
};
pub(super) const PROCESS_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: PROCESS_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: PROCESS_CONFIG_ROOT_MESSAGE,
        emit: emit_typed_payload::<ProcessConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: PROCESS_DATA_ROOT_MESSAGE,
        emit: emit_typed_payload::<ProcessData>,
    },
};
pub(super) const DISKIO_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: DISKIO_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: DISKIO_CONFIG_ROOT_MESSAGE,
        emit: emit_typed_payload::<DiskioConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: DISKIO_DATA_ROOT_MESSAGE,
        emit: emit_typed_payload::<DiskioData>,
    },
};
pub(super) const NETWORK_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: NETWORK_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: NETWORK_CONFIG_ROOT_MESSAGE,
        emit: emit_typed_payload::<NetworkConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: NETWORK_DATA_ROOT_MESSAGE,
        emit: emit_typed_payload::<NetworkDatas>,
    },
};
pub(super) const GPU_ROUTE: ProfilerPluginRoute = ProfilerPluginRoute {
    plugin_name: GPU_PLUGIN_NAME,
    config: Some(ProfilerPayloadRoute {
        root_message: GPU_CONFIG_ROOT_MESSAGE,
        emit: emit_typed_payload::<GpuConfig>,
    }),
    data: ProfilerPayloadRoute {
        root_message: GPU_DATA_ROOT_MESSAGE,
        emit: emit_typed_payload::<GpuData>,
    },
};
