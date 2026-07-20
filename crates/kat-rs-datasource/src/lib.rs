mod dataset;
mod decode;
mod formats;
mod json;
mod materializer;
mod mmap;
mod payload_value;
mod query;
mod record;
mod relational;

pub use dataset::{
    DatasetLocator, DatasetResolution, DatasetStore, DatasetTableInfo, inspect_dataset_tables,
    write_derived_dataset_table,
};
pub use materializer::{materialize_hitrace_dataset, materialize_langfuse_legacy_dataset};
pub use query::TraceDatasource;

#[doc(hidden)]
pub mod relational_for_tests {
    pub fn descriptor_root_names() -> Vec<String> {
        crate::relational::descriptor::descriptor_root_names()
    }

    pub fn expansion_plan_table_names(root_messages: &[&str]) -> Vec<String> {
        crate::relational::plan::expansion_plan_for_roots(root_messages)
            .into_iter()
            .map(|item| item.output_table)
            .collect()
    }
}

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
