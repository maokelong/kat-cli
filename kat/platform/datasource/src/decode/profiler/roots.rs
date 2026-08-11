pub(crate) const CPU_CONFIG_ROOT_MESSAGE: &str = "CpuConfig";
pub(crate) const CPU_DATA_ROOT_MESSAGE: &str = "CpuData";
pub(crate) const MEMORY_CONFIG_ROOT_MESSAGE: &str = "MemoryConfig";
pub(crate) const MEMORY_DATA_ROOT_MESSAGE: &str = "MemoryData";
pub(crate) const PROCESS_CONFIG_ROOT_MESSAGE: &str = "ProcessConfig";
pub(crate) const PROCESS_DATA_ROOT_MESSAGE: &str = "ProcessData";
pub(crate) const DISKIO_CONFIG_ROOT_MESSAGE: &str = "DiskioConfig";
pub(crate) const DISKIO_DATA_ROOT_MESSAGE: &str = "DiskioData";
pub(crate) const NETWORK_CONFIG_ROOT_MESSAGE: &str = "NetworkConfig";
pub(crate) const NETWORK_DATA_ROOT_MESSAGE: &str = "NetworkDatas";
pub(crate) const GPU_CONFIG_ROOT_MESSAGE: &str = "GpuConfig";
pub(crate) const GPU_DATA_ROOT_MESSAGE: &str = "GpuData";
pub(crate) const FTRACE_ROOT_MESSAGE: &str = "TracePluginResult";
pub(crate) const NATIVE_HOOK_CONFIG_ROOT_MESSAGE: &str = "NativeHookConfig";
pub(crate) const NATIVE_HOOK_DATA_ROOT_MESSAGE: &str = "BatchNativeHookData";

pub(crate) const RELATIONAL_ROOT_MESSAGES: &[&str] = &[
    CPU_CONFIG_ROOT_MESSAGE,
    CPU_DATA_ROOT_MESSAGE,
    MEMORY_CONFIG_ROOT_MESSAGE,
    MEMORY_DATA_ROOT_MESSAGE,
    PROCESS_CONFIG_ROOT_MESSAGE,
    PROCESS_DATA_ROOT_MESSAGE,
    DISKIO_CONFIG_ROOT_MESSAGE,
    DISKIO_DATA_ROOT_MESSAGE,
    NETWORK_CONFIG_ROOT_MESSAGE,
    NETWORK_DATA_ROOT_MESSAGE,
    GPU_CONFIG_ROOT_MESSAGE,
    GPU_DATA_ROOT_MESSAGE,
    FTRACE_ROOT_MESSAGE,
    NATIVE_HOOK_CONFIG_ROOT_MESSAGE,
    NATIVE_HOOK_DATA_ROOT_MESSAGE,
];
