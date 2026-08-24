//! KAT 的 Hitrace 解码与 Dataset Binding/Storage package。
//!
//! 当前切片提供 Hitrace 来源解析、Dataset inspection 与受管 Dataset 写入能力。

// 这些内部 Arrow 投影仍由 Hitrace 领域合同覆盖；首个 Source 切片只发布其中的稳定子集。
#[allow(dead_code)]
mod arrow_table;
mod dataset;
mod dataset_writer;
#[allow(dead_code)]
mod domains;
#[allow(dead_code)]
mod formats;
#[allow(dead_code)]
mod ftrace_event_table_builders {
    include!(concat!(env!("OUT_DIR"), "/ftrace_event_table_builders.rs"));
}
#[allow(dead_code)]
mod json;
mod materializer;
mod mmap;
#[allow(dead_code)]
mod native_hook_table_builders {
    include!(concat!(env!("OUT_DIR"), "/native_hook_table_builders.rs"));
}
#[allow(dead_code)]
mod protobuf_source;
#[cfg(all(test, feature = "protobuf-source-contract-fixture"))]
#[path = "../build/protobuf_source_codegen/mod.rs"]
mod protobuf_source_codegen;
#[cfg(all(test, feature = "protobuf-source-contract-fixture"))]
#[path = "../tests/protobuf_source_contract/mod.rs"]
mod protobuf_source_contract_tests;
#[allow(dead_code)]
mod record;
#[allow(dead_code)]
mod sinks;
mod table_name;

pub use dataset::{
    ColumnInspection, DatasetBindingKind, DatasetInspection, DatasetInspectionError,
    DatasetMutationError, DatasetTargetInspection, MaterializedSourcePublication, ResolvedDataset,
    ResolvedSource, ResolvedTable, SourceInspection, TableInspection, inspect_dataset,
    inspect_dataset_target, publish_materialized_source, resolve_dataset, write_external_binding,
};
pub use materializer::{
    HitraceStagingError, StagedHitrace, UnsupportedHitraceContent, stage_hitrace,
};

pub(crate) use table_name::valid_table_name;

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

    #[cfg(all(test, feature = "protobuf-source-contract-fixture"))]
    include!(concat!(
        env!("OUT_DIR"),
        "/protobuf_source_fixture/fixture_proto.rs"
    ));
}

#[cfg(all(test, feature = "protobuf-source-contract-fixture"))]
mod generated_fixture_emitter {
    include!(concat!(
        env!("OUT_DIR"),
        "/protobuf_source_fixture/fixture_emitter.rs"
    ));
}

#[allow(dead_code)]
mod generated_profiler_source_emitter {
    include!(concat!(env!("OUT_DIR"), "/profiler_source_emitter.rs"));
}
