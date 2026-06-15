mod catalog;
mod domains;
mod formats;
mod ftrace_event_table_builders {
    include!(concat!(env!("OUT_DIR"), "/ftrace_event_table_builders.rs"));
}
mod json;
mod mmap;
mod plugin_flow;
mod query;
mod record;
mod sinks;

pub use query::TraceDatasource;

#[allow(dead_code)]
pub(crate) mod proto {
    pub(crate) mod kat {
        pub(crate) mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }
    }

    pub(crate) use kat::hitrace::{ProfilerPluginData, TracePluginResult};
}
