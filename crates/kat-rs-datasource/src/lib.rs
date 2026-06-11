mod hitrace;
mod json;
mod mmap;
mod query;

pub use query::TraceDatasource;

#[allow(dead_code)]
pub(crate) mod proto {
    include!(concat!(env!("OUT_DIR"), "/_.rs"));

    pub(crate) mod kat {
        pub(crate) mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }
    }

    pub(crate) use kat::hitrace::{ProfilerPluginData, TracePluginResult};
}
