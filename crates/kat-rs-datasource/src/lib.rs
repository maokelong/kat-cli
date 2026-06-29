mod arrow_table;
mod dataset;
mod domains;
mod formats;
mod fixed_result_table_builders {
    include!(concat!(env!("OUT_DIR"), "/fixed_result_table_builders.rs"));
}
mod ftrace_event_table_builders {
    include!(concat!(env!("OUT_DIR"), "/ftrace_event_table_builders.rs"));
}
mod json;
mod materializer;
mod mmap;
mod native_hook_table_builders {
    include!(concat!(env!("OUT_DIR"), "/native_hook_table_builders.rs"));
}
mod query;
mod record;
mod sinks;

pub use dataset::{
    DatasetLocator, DatasetResolution, DatasetStore, DatasetTableInfo, inspect_dataset_tables,
    write_derived_dataset_table,
};
pub use materializer::{materialize_hitrace_dataset, materialize_langfuse_legacy_dataset};
pub use query::TraceDatasource;

#[allow(dead_code)]
pub(crate) mod proto {
    pub(crate) mod kat {
        pub(crate) mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }

        pub(crate) mod native_hook {
            include!(concat!(env!("OUT_DIR"), "/kat.native_hook.rs"));
        }

        pub(crate) mod cpu_data {
            include!(concat!(env!("OUT_DIR"), "/kat.cpu_data.rs"));
        }

        pub(crate) mod memory_data {
            include!(concat!(env!("OUT_DIR"), "/kat.memory_data.rs"));
        }

        pub(crate) mod process_data {
            include!(concat!(env!("OUT_DIR"), "/kat.process_data.rs"));
        }

        pub(crate) mod diskio_data {
            include!(concat!(env!("OUT_DIR"), "/kat.diskio_data.rs"));
        }

        pub(crate) mod network_data {
            include!(concat!(env!("OUT_DIR"), "/kat.network_data.rs"));
        }

        pub(crate) mod gpu_data {
            include!(concat!(env!("OUT_DIR"), "/kat.gpu_data.rs"));
        }
    }

    pub(crate) use kat::hitrace::{ProfilerPluginData, TracePluginResult};
    pub(crate) use kat::native_hook::{BatchNativeHookData, NativeHookConfig};
}
