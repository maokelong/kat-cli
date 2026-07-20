#[path = "build/relational_descriptor_codegen.rs"]
mod relational_descriptor_codegen;

use prost_types::{
    DescriptorProto, FieldDescriptorProto, FileDescriptorSet,
    field_descriptor_proto::{Label, Type},
};

use relational_descriptor_codegen::generate_relational_descriptors;

const SERDE_DERIVE: &str = "#[derive(serde::Serialize, serde::Deserialize)]";
const CLIPPY_ENUM_VARIANT_NAMES: &str = "#[allow(clippy::enum_variant_names)]";
const SERDE_SKIP_OPTION_NONE: &str = "#[serde(skip_serializing_if = \"Option::is_none\")]";
const SERDE_SKIP_VEC_EMPTY: &str = "#[serde(skip_serializing_if = \"Vec::is_empty\")]";

const FTRACE_PROTO_FILES: &[&str] = &[
    "proto/ftrace_data/binder.proto",
    "proto/ftrace_data/block.proto",
    "proto/ftrace_data/cgroup.proto",
    "proto/ftrace_data/clk.proto",
    "proto/ftrace_data/compaction.proto",
    "proto/ftrace_data/cpuhp.proto",
    "proto/ftrace_data/dma_fence.proto",
    "proto/ftrace_data/ext4.proto",
    "proto/ftrace_data/f2fs.proto",
    "proto/ftrace_data/filelock.proto",
    "proto/ftrace_data/filemap.proto",
    "proto/ftrace_data/ftrace.proto",
    "proto/ftrace_data/ftrace_event.proto",
    "proto/ftrace_data/gpio.proto",
    "proto/ftrace_data/gpu_mem.proto",
    "proto/ftrace_data/i2c.proto",
    "proto/ftrace_data/ipi.proto",
    "proto/ftrace_data/irq.proto",
    "proto/ftrace_data/kmem.proto",
    "proto/ftrace_data/mmc.proto",
    "proto/ftrace_data/net.proto",
    "proto/ftrace_data/oom.proto",
    "proto/ftrace_data/pagemap.proto",
    "proto/ftrace_data/power.proto",
    "proto/ftrace_data/printk.proto",
    "proto/ftrace_data/raw_syscalls.proto",
    "proto/ftrace_data/rcu.proto",
    "proto/ftrace_data/regulator.proto",
    "proto/ftrace_data/sched.proto",
    "proto/ftrace_data/signal.proto",
    "proto/ftrace_data/sunrpc.proto",
    "proto/ftrace_data/task.proto",
    "proto/ftrace_data/timer.proto",
    "proto/ftrace_data/trace_plugin_config.proto",
    "proto/ftrace_data/trace_plugin_result.proto",
    "proto/ftrace_data/v4l2.proto",
    "proto/ftrace_data/vmscan.proto",
    "proto/ftrace_data/workqueue.proto",
    "proto/ftrace_data/writeback.proto",
];
const NATIVE_HOOK_PROTO_FILES: &[&str] = &[
    "proto/native_hook/native_hook_config.proto",
    "proto/native_hook/native_hook_result.proto",
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
        .chain(FTRACE_PROTO_FILES.iter().copied())
        .chain(NATIVE_HOOK_PROTO_FILES.iter().copied())
        .chain(FIXED_RESULT_PROTO_FILES.iter().copied())
        .collect::<Vec<_>>();
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    let fds = config
        .load_fds(&proto_files, &["proto"])
        .expect("proto descriptors load");
    generate_relational_descriptors(&fds).expect("relational descriptors are written");
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
        config.type_attribute(&package, SERDE_DERIVE);
        config.enum_attribute(&package, CLIPPY_ENUM_VARIANT_NAMES);
    }
    add_sparse_serde_field_attributes(&mut config, &fds);
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
    for proto_file in proto_files {
        println!("cargo:rerun-if-changed={proto_file}");
    }
}

fn add_sparse_serde_field_attributes(config: &mut prost_build::Config, fds: &FileDescriptorSet) {
    for file in &fds.file {
        let package = file.package.as_deref().unwrap_or("");
        for message in &file.message_type {
            add_sparse_message_field_attributes(config, package, &[], message);
        }
    }
}

fn add_sparse_message_field_attributes(
    config: &mut prost_build::Config,
    package: &str,
    parent_messages: &[&str],
    message: &DescriptorProto,
) {
    let Some(message_name) = message.name.as_deref() else {
        return;
    };

    let mut message_path = parent_messages.to_vec();
    message_path.push(message_name);

    for field in &message.field {
        add_sparse_field_attribute(config, package, &message_path, field);
    }

    for nested in &message.nested_type {
        add_sparse_message_field_attributes(config, package, &message_path, nested);
    }
}

fn add_sparse_field_attribute(
    config: &mut prost_build::Config,
    package: &str,
    message_path: &[&str],
    field: &FieldDescriptorProto,
) {
    let Some(field_name) = field.name.as_deref() else {
        return;
    };

    let field_path = format!(
        "{}.{}",
        qualified_type_name(package, message_path),
        field_name
    );
    match field.label.and_then(|label| Label::try_from(label).ok()) {
        Some(Label::Repeated) => {
            config.field_attribute(&field_path, SERDE_SKIP_VEC_EMPTY);
        }
        _ if field
            .r#type
            .and_then(|field_type| Type::try_from(field_type).ok())
            == Some(Type::Message) =>
        {
            config.field_attribute(&field_path, SERDE_SKIP_OPTION_NONE);
        }
        _ => {}
    }
}

fn qualified_type_name(package: &str, message_path: &[&str]) -> String {
    if package.is_empty() {
        format!(".{}", message_path.join("."))
    } else {
        format!(".{}.{}", package, message_path.join("."))
    }
}
