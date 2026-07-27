#[path = "build/ftrace_arrow_codegen.rs"]
mod ftrace_arrow_codegen;
#[path = "build/native_hook_arrow_codegen.rs"]
mod native_hook_arrow_codegen;
#[path = "build/native_hook_domain_codegen.rs"]
mod native_hook_domain_codegen;
#[path = "build/proto_codegen.rs"]
mod proto_codegen;
#[path = "build/relational_descriptor_codegen.rs"]
mod relational_descriptor_codegen;

use ftrace_arrow_codegen::{
    EventFamily, FTRACE_EVENT_FAMILIES, generate_ftrace_event_table_builders,
};
use native_hook_arrow_codegen::generate_native_hook_table_builders;
use native_hook_domain_codegen::{
    NATIVE_HOOK_PROTO_FILES, NATIVE_HOOK_RESULT_PROTO, generate_native_hook_records,
    native_hook_events_from_descriptor,
};
use proto_codegen::{message_in_file, messages_in_file};
use relational_descriptor_codegen::generate_relational_descriptors;

const SERDE_DERIVE: &str = "#[derive(serde::Serialize, serde::Deserialize)]";
const CLIPPY_ENUM_VARIANT_NAMES: &str = "#[allow(clippy::enum_variant_names)]";

const FTRACE_PAYLOAD_PROTO_FILES: &[&str] = &[
    "proto/ftrace_data/ftrace_event.proto",
    "proto/ftrace_data/trace_plugin_result.proto",
];
const FIXED_RESULT_PROTO_FILES: &[&str] = &[
    "proto/cpu_data/cpu_plugin_config.proto",
    "proto/cpu_data/cpu_plugin_result.proto",
    "proto/memory_data/memory_plugin_common.proto",
    "proto/memory_data/memory_plugin_config.proto",
    "proto/memory_data/memory_plugin_result.proto",
    "proto/process_data/process_plugin_config.proto",
    "proto/process_data/process_plugin_result.proto",
    "proto/diskio_data/diskio_plugin_config.proto",
    "proto/diskio_data/diskio_plugin_result.proto",
    "proto/network_data/network_plugin_config.proto",
    "proto/network_data/network_plugin_result.proto",
    "proto/gpu_data/gpu_plugin_config.proto",
    "proto/gpu_data/gpu_plugin_result.proto",
];
const FIXED_RESULT_PROTO_PACKAGES: &[&str] = &[
    ".kat.cpu_data",
    ".kat.memory_data",
    ".kat.process_data",
    ".kat.diskio_data",
    ".kat.network_data",
    ".kat.gpu_data",
];
const PROFILER_ENVELOPE_PROTO_FILES: &[&str] = &["proto/profiler/profiler_plugin_data.proto"];

fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc is available");
    let proto_files = PROFILER_ENVELOPE_PROTO_FILES
        .iter()
        .copied()
        .chain(FTRACE_PAYLOAD_PROTO_FILES.iter().copied())
        .chain(FTRACE_EVENT_FAMILIES.iter().map(|family| family.proto_path))
        .chain(NATIVE_HOOK_PROTO_FILES.iter().copied())
        .chain(FIXED_RESULT_PROTO_FILES.iter().copied())
        .collect::<Vec<_>>();
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    let fds = config
        .load_fds(&proto_files, &["proto"])
        .expect("proto descriptors load");
    generate_relational_descriptors(&fds).expect("relational descriptors are written");
    let event_families = FTRACE_EVENT_FAMILIES
        .iter()
        .map(|spec| EventFamily {
            spec,
            messages: messages_in_file(&fds, spec.proto_path),
        })
        .collect::<Vec<_>>();
    let native_hook_messages = messages_in_file(&fds, NATIVE_HOOK_RESULT_PROTO);
    let native_hook_data = message_in_file(&fds, NATIVE_HOOK_RESULT_PROTO, "NativeHookData");
    let native_hook_events =
        native_hook_events_from_descriptor(native_hook_data, &native_hook_messages);

    config.type_attribute(".kat.hitrace", SERDE_DERIVE);
    config.enum_attribute(
        ".kat.hitrace.ProfilerPluginData.ClockId",
        CLIPPY_ENUM_VARIANT_NAMES,
    );
    config.type_attribute(".kat.native_hook", SERDE_DERIVE);
    config.enum_attribute(
        ".kat.native_hook.NativeHookData.event",
        CLIPPY_ENUM_VARIANT_NAMES,
    );
    for package in FIXED_RESULT_PROTO_PACKAGES {
        config.type_attribute(package, SERDE_DERIVE);
        config.enum_attribute(package, CLIPPY_ENUM_VARIANT_NAMES);
    }
    config.field_attribute(
        ".kat.hitrace.ProfilerPluginData.data",
        "#[serde(with = \"serde_bytes\")]",
    );
    config.field_attribute(
        ".kat.native_hook.SymbolTable.sym_table",
        "#[serde(with = \"serde_bytes\")]",
    );
    config.field_attribute(
        ".kat.native_hook.SymbolTable.str_table",
        "#[serde(with = \"serde_bytes\")]",
    );
    config
        .compile_fds(fds)
        .expect("hitrace and event family protos compile");
    generate_ftrace_event_table_builders(&event_families)
        .expect("ftrace event table builders are written");
    generate_native_hook_records(&native_hook_events).expect("native hook records are written");
    generate_native_hook_table_builders(&native_hook_events)
        .expect("native hook table builders are written");

    for proto_file in proto_files {
        println!("cargo:rerun-if-changed={proto_file}");
    }
}
