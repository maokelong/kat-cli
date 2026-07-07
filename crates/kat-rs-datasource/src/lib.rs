mod arrow_table;
mod dataset;
mod domains;
mod formats;
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
pub use materializer::{
    materialize_hitrace_dataset, materialize_langfuse_legacy_dataset,
    materialize_sqlite_pack_demo_dataset,
};
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
    }

    pub(crate) use kat::hitrace::{ProfilerPluginData, TracePluginResult};
    pub(crate) use kat::native_hook::{BatchNativeHookData, NativeHookConfig};
}
