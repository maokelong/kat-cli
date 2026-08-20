#[path = "build/ftrace_arrow_codegen.rs"]
mod ftrace_arrow_codegen;
#[path = "build/native_hook_arrow_codegen.rs"]
mod native_hook_arrow_codegen;
#[path = "build/native_hook_domain_codegen.rs"]
mod native_hook_domain_codegen;
#[path = "build/proto_codegen.rs"]
mod proto_codegen;
#[path = "build/protobuf_source_codegen/mod.rs"]
mod protobuf_source_codegen;
#[path = "src/table_name.rs"]
mod table_name;

use std::{env, fs, path::Path};

use ftrace_arrow_codegen::{
    EventFamily, FTRACE_EVENT_FAMILIES, generate_ftrace_event_table_builders,
};
use native_hook_arrow_codegen::generate_native_hook_table_builders;
use native_hook_domain_codegen::{
    NATIVE_HOOK_PROTO_FILES, NATIVE_HOOK_RESULT_PROTO, generate_native_hook_records,
    native_hook_events_from_descriptor, native_hook_serializable_messages,
};
#[cfg(feature = "protobuf-source-contract-fixture")]
use prost::Message;
use proto_codegen::{message_in_file, messages_in_file};
#[cfg(feature = "protobuf-source-contract-fixture")]
use protobuf_source_codegen::compile as compile_protobuf_source;
use protobuf_source_codegen::{RootSpec, compile_for_profiler_capture};

const FTRACE_PAYLOAD_PROTO_FILES: &[&str] = &[
    "proto/ftrace_data/ftrace.proto",
    "proto/ftrace_data/ipi.proto",
    "proto/ftrace_data/irq.proto",
    "proto/ftrace_data/kmem.proto",
    "proto/ftrace_data/vmscan.proto",
    "proto/ftrace_data/workqueue.proto",
    "proto/ftrace_data/ftrace_event.proto",
    "proto/ftrace_data/trace_plugin_config.proto",
    "proto/ftrace_data/trace_plugin_result.proto",
];
const PROFILER_ENVELOPE_PROTO_FILES: &[&str] = &["proto/profiler/profiler_plugin_data.proto"];
#[cfg(feature = "protobuf-source-contract-fixture")]
const PROTOBUF_SOURCE_FIXTURE_DIR: &str = "tests/fixtures/protobuf_source";
#[cfg(feature = "protobuf-source-contract-fixture")]
const PROTOBUF_SOURCE_VALID_FIXTURES: &[&str] = &[
    "tests/fixtures/protobuf_source/valid_shapes.proto",
    "tests/fixtures/protobuf_source/valid_proto2_optional.proto",
    "tests/fixtures/protobuf_source/same_name_alpha.proto",
    "tests/fixtures/protobuf_source/same_name_beta.proto",
    "tests/fixtures/protobuf_source/valid_naming.proto",
    "tests/fixtures/protobuf_source/illegal_field_names.proto",
];
#[cfg(feature = "protobuf-source-contract-fixture")]
const PROTOBUF_SOURCE_PLANNER_FIXTURES: &[&str] = &[
    "tests/fixtures/protobuf_source/name_collision.proto",
    "tests/fixtures/protobuf_source/reserved_relation_names.proto",
    "tests/fixtures/protobuf_source/unsupported_enum_alias.proto",
    "tests/fixtures/protobuf_source/unsupported_extension.proto",
    "tests/fixtures/protobuf_source/unsupported_group.proto",
    "tests/fixtures/protobuf_source/unsupported_map.proto",
    "tests/fixtures/protobuf_source/unsupported_recursive.proto",
    "tests/fixtures/protobuf_source/unsupported_required.proto",
];

fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc is available");
    let proto_files = PROFILER_ENVELOPE_PROTO_FILES
        .iter()
        .copied()
        .chain(FTRACE_PAYLOAD_PROTO_FILES.iter().copied())
        .chain(FTRACE_EVENT_FAMILIES.iter().map(|family| family.proto_path))
        .chain(NATIVE_HOOK_PROTO_FILES.iter().copied())
        .collect::<Vec<_>>();
    let mut config = prost_build::Config::new();
    config.protoc_executable(&protoc);
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
    config.enum_attribute(
        ".kat.hitrace.FtraceEvent.event",
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
    generate_profiler_source_emitter(&fds);
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
    #[cfg(feature = "protobuf-source-contract-fixture")]
    generate_protobuf_source_fixture(&protoc);

    for proto_file in proto_files {
        println!("cargo:rerun-if-changed={proto_file}");
    }
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
            RootSpec::new("kat.hitrace.TracePluginResult", "trace_plugin_result")
                .with_nullable_parent()
                .with_incremental_relations(&["ftrace_cpu_detail", "ftrace_cpu_detail.event"]),
            RootSpec::new("kat.hitrace.TracePluginConfig", "trace_plugin_config")
                .with_nullable_parent(),
        ],
    )
    .expect("profiler protobuf Source roots compile")
    .with_enum_symbol_accessor(
        descriptors,
        "kat.hitrace.ProfilerPluginData.ClockId",
        "profiler_clock_id_symbols",
    )
    .expect("ProfilerPluginData ClockId symbols compile");
    let output = Path::new(&env::var_os("OUT_DIR").expect("Cargo provides OUT_DIR"))
        .join("profiler_source_emitter.rs");
    fs::write(output, generated.into_source())
        .expect("profiler protobuf Source emitter is written");
}

#[cfg(feature = "protobuf-source-contract-fixture")]
fn generate_protobuf_source_fixture(protoc: &Path) {
    let output = Path::new(&env::var_os("OUT_DIR").expect("Cargo provides OUT_DIR"))
        .join("protobuf_source_fixture");
    fs::create_dir_all(&output).expect("protobuf Source fixture output directory is created");

    let all_files = PROTOBUF_SOURCE_VALID_FIXTURES
        .iter()
        .chain(PROTOBUF_SOURCE_PLANNER_FIXTURES)
        .copied()
        .collect::<Vec<_>>();
    let all_descriptors = load_fixture_descriptors(protoc, &all_files);
    fs::write(
        output.join("all_descriptors.bin"),
        all_descriptors.encode_to_vec(),
    )
    .expect("protobuf Source planner fixture descriptors are written");

    let valid_descriptors = load_fixture_descriptors(protoc, PROTOBUF_SOURCE_VALID_FIXTURES);
    let mut prost_config = prost_build::Config::new();
    prost_config.protoc_executable(protoc);
    prost_config.out_dir(&output);
    prost_config.include_file("fixture_proto.rs");
    prost_config.enum_attribute(
        ".fixture.protobuf_source.valid.OneofMatrix.selected",
        "#[allow(clippy::enum_variant_names)]",
    );
    prost_config
        .compile_fds(valid_descriptors.clone())
        .expect("valid protobuf Source fixture types compile");

    let generated = compile_protobuf_source(
        &valid_descriptors,
        &[
            RootSpec::new(
                "fixture.protobuf_source.valid.FullShapeRoot",
                "full_shape_root",
            ),
            RootSpec::new(
                "fixture.protobuf_source.valid.ScalarMatrix",
                "scalar_matrix",
            ),
            RootSpec::new(
                "fixture.protobuf_source.valid.InlineOneofRoot",
                "inline_oneof_root",
            ),
            RootSpec::new(
                "fixture.protobuf_source.valid.Proto2OptionalRoot",
                "proto2_optional_root",
            ),
            RootSpec::new(
                "fixture.protobuf_source.valid.DeepRepeatedRoot",
                "deep_repeated_root",
            )
            .with_incremental_relations(&["containers", "containers.children"]),
            RootSpec::new("fixture.protobuf_source.valid.EmptyRoot", "empty_root"),
            RootSpec::new(
                "fixture.protobuf_source.alpha.SharedRoot",
                "alpha_shared_root",
            ),
            RootSpec::new(
                "fixture.protobuf_source.beta.SharedRoot",
                "beta_shared_root",
            ),
            RootSpec::new(
                "fixture.protobuf_source.naming.KeywordAcronymRoot",
                "keyword_acronym_root",
            ),
            RootSpec::new(
                "fixture.protobuf_source.naming.NestedOneofRoot",
                "nested_oneof_root",
            ),
            RootSpec::new(
                "fixture.protobuf_source.naming.OneofNestedNameRoot",
                "oneof_nested_name_root",
            ),
            RootSpec::new(
                "fixture.protobuf_source.illegal_field_names.UppercaseFieldRoot",
                "uppercase_field_root",
            ),
            RootSpec::new(
                "fixture.protobuf_source.illegal_field_names.NestedFieldNameRoot",
                "nested_field_name_root",
            ),
        ],
    )
    .expect("valid protobuf Source fixture plan compiles");
    fs::write(output.join("fixture_emitter.rs"), generated.into_source())
        .expect("protobuf Source fixture emitter is written");

    for proto_file in all_files {
        println!("cargo:rerun-if-changed={proto_file}");
    }
}

#[cfg(feature = "protobuf-source-contract-fixture")]
fn load_fixture_descriptors(protoc: &Path, proto_files: &[&str]) -> prost_types::FileDescriptorSet {
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config
        .load_fds(proto_files, &[PROTOBUF_SOURCE_FIXTURE_DIR])
        .expect("protobuf Source fixture descriptors load")
}
