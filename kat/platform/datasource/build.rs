#[path = "build/protobuf_source_codegen/mod.rs"]
mod protobuf_source_codegen;
#[path = "src/relation_name.rs"]
mod relation_name;

use std::{env, fs, path::Path};

use protobuf_source_codegen::{RootSpec, compile_for_profiler_capture};

const PROTO_FILES: &[&str] = &[
    "proto/text_ftrace/text_ftrace_event.proto",
    "proto/profiler/profiler_plugin_data.proto",
    "proto/ftrace_data/ftrace_event.proto",
    "proto/ftrace_data/trace_plugin_config.proto",
    "proto/ftrace_data/trace_plugin_result.proto",
    "proto/native_hook/native_hook_config.proto",
    "proto/native_hook/native_hook_result.proto",
];

fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc is available");
    let mut config = prost_build::Config::new();
    config.protoc_executable(&protoc);
    let descriptors = config
        .load_fds(PROTO_FILES, &["proto"])
        .expect("Hitrace descriptor closure loads");

    for descriptor in &descriptors.file {
        if let Some(name) = descriptor.name.as_deref() {
            println!(
                "cargo:rerun-if-changed={}",
                Path::new("proto").join(name).display()
            );
        }
    }

    config.enum_attribute(
        ".kat.hitrace.ProfilerPluginData.ClockId",
        "#[allow(clippy::enum_variant_names)]",
    );
    config.enum_attribute(
        ".kat.native_hook.NativeHookData.event",
        "#[allow(clippy::enum_variant_names)]",
    );
    config.enum_attribute(
        ".kat.hitrace.FtraceEvent.event",
        "#[allow(clippy::enum_variant_names)]",
    );

    generate_profiler_source_emitter(&descriptors);
    config
        .compile_fds(descriptors)
        .expect("Hitrace descriptor closure compiles");
}

fn generate_profiler_source_emitter(descriptors: &prost_types::FileDescriptorSet) {
    let generated = compile_for_profiler_capture(
        descriptors,
        &[
            RootSpec::new(
                "kat.native_hook.BatchNativeHookData",
                "batch_native_hook_data",
            ),
            RootSpec::new("kat.native_hook.NativeHookConfig", "native_hook_config"),
            RootSpec::new("kat.hitrace.TracePluginResult", "trace_plugin_result"),
            RootSpec::new("kat.hitrace.TracePluginConfig", "trace_plugin_config"),
        ],
    )
    .expect("profiler descriptor roots compile")
    .with_enum_symbol_accessor(
        descriptors,
        "kat.hitrace.ProfilerPluginData.ClockId",
        "profiler_clock_id_symbols",
    )
    .expect("ProfilerPluginData ClockId symbols compile");
    let output = Path::new(&env::var_os("OUT_DIR").expect("Cargo provides OUT_DIR"))
        .join("profiler_source_emitter.rs");
    fs::write(output, generated.into_source())
        .expect("profiler descriptor relation emitter is written");
}
