#[path = "build/fixed_result_arrow_codegen.rs"]
mod fixed_result_arrow_codegen;
#[path = "build/fixed_result_domain_codegen.rs"]
mod fixed_result_domain_codegen;
#[path = "build/ftrace_arrow_codegen.rs"]
mod ftrace_arrow_codegen;
#[path = "build/native_hook_arrow_codegen.rs"]
mod native_hook_arrow_codegen;
#[path = "build/native_hook_domain_codegen.rs"]
mod native_hook_domain_codegen;
#[path = "build/proto_codegen.rs"]
mod proto_codegen;

use fixed_result_arrow_codegen::generate_fixed_result_table_builders;
use fixed_result_domain_codegen::{
    FIXED_RESULT_PLUGIN_SPECS, FIXED_RESULT_PROTO_FILES, fixed_result_enum_paths,
    fixed_result_serializable_message_paths, generate_fixed_result_records,
};
use ftrace_arrow_codegen::{
    EventFamily, FTRACE_EVENT_FAMILIES, generate_ftrace_event_table_builders,
};
use native_hook_arrow_codegen::generate_native_hook_table_builders;
use native_hook_domain_codegen::{
    NATIVE_HOOK_PROTO_FILES, NATIVE_HOOK_RESULT_PROTO, generate_native_hook_records,
    native_hook_events_from_descriptor, native_hook_serializable_messages,
};
use proto_codegen::{message_in_file, messages_in_file};

const FTRACE_PAYLOAD_PROTO_FILES: &[&str] = &[
    "proto/ftrace_data/ftrace_event.proto",
    "proto/ftrace_data/trace_plugin_result.proto",
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

    config.type_attribute(
        ".kat.hitrace.ProfilerPluginData",
        "#[derive(serde::Serialize, serde::Deserialize)]",
    );
    config.enum_attribute(
        ".kat.hitrace.ProfilerPluginData.ClockId",
        "#[allow(clippy::enum_variant_names)]",
    );
    config.type_attribute(
        ".kat.native_hook.NativeHookConfig",
        "#[derive(serde::Serialize, serde::Deserialize)]",
    );
    config.enum_attribute(
        ".kat.native_hook.NativeHookData.event",
        "#[allow(clippy::enum_variant_names)]",
    );
    for message_name in
        native_hook_serializable_messages(&native_hook_messages, &native_hook_events)
    {
        let path = format!(".kat.native_hook.{message_name}");
        config.type_attribute(&path, "#[derive(serde::Serialize, serde::Deserialize)]");
    }
    for family in &event_families {
        for message in &family.messages {
            let path = format!(".kat.hitrace.{}", message.name);
            config.type_attribute(&path, "#[derive(serde::Serialize, serde::Deserialize)]");
        }
    }
    for message_path in fixed_result_serializable_message_paths(&fds) {
        config.type_attribute(
            &message_path,
            "#[derive(serde::Serialize, serde::Deserialize)]",
        );
    }
    for enum_path in fixed_result_enum_paths(&fds) {
        config.enum_attribute(&enum_path, "#[allow(clippy::enum_variant_names)]");
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
    generate_fixed_result_records(FIXED_RESULT_PLUGIN_SPECS)
        .expect("fixed result records are written");
    generate_fixed_result_table_builders(FIXED_RESULT_PLUGIN_SPECS)
        .expect("fixed result table builders are written");

    for proto_file in proto_files {
        println!("cargo:rerun-if-changed={proto_file}");
    }
}
