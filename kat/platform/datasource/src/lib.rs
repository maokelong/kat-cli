//! Hitrace decoder backing the private Python native extension.

mod formats;
mod hitrace_decode;
mod mmap;
mod protobuf_source;
#[cfg(feature = "python-extension")]
mod python;
mod relation_name;
mod relation_writer;

pub use hitrace_decode::{HitraceDecodeError, HitraceDecodeReport, decode_hitrace};

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

    pub(crate) use kat::hitrace::{ProfilerPluginData, TracePluginConfig, TracePluginResult};
    pub(crate) use kat::native_hook::{BatchNativeHookData, NativeHookConfig};
}

mod generated_profiler_source_emitter {
    include!(concat!(env!("OUT_DIR"), "/profiler_source_emitter.rs"));
}
