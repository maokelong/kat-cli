use std::{env, fmt::Write as _, fs, path::PathBuf};

use prost_types::{
    DescriptorProto, FieldDescriptorProto, FileDescriptorSet,
    field_descriptor_proto::{Label, Type},
};

use crate::proto_codegen::{proto_file, proto_message_to_rust_type, snake_to_upper_camel};

const FIXED_RESULT_RECORDS_FILE: &str = "fixed_result_records.rs";

pub(crate) const FIXED_RESULT_PROTO_FILES: &[&str] = &[
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

pub(crate) const FIXED_RESULT_PLUGIN_SPECS: &[FixedResultPluginSpec] = &[
    FixedResultPluginSpec {
        module_name: "cpu_data",
        plugin_name: "cpu-plugin",
        config_message: "CpuConfig",
        result_message: "CpuData",
        config_table: "cpu_config",
        result_table: "cpu_data",
    },
    FixedResultPluginSpec {
        module_name: "memory_data",
        plugin_name: "memory-plugin",
        config_message: "MemoryConfig",
        result_message: "MemoryData",
        config_table: "memory_config",
        result_table: "memory_data",
    },
    FixedResultPluginSpec {
        module_name: "process_data",
        plugin_name: "process-plugin",
        config_message: "ProcessConfig",
        result_message: "ProcessData",
        config_table: "process_config",
        result_table: "process_data",
    },
    FixedResultPluginSpec {
        module_name: "diskio_data",
        plugin_name: "diskio-plugin",
        config_message: "DiskioConfig",
        result_message: "DiskioData",
        config_table: "diskio_config",
        result_table: "diskio_data",
    },
    FixedResultPluginSpec {
        module_name: "network_data",
        plugin_name: "network-plugin",
        config_message: "NetworkConfig",
        result_message: "NetworkDatas",
        config_table: "network_config",
        result_table: "network_data",
    },
    FixedResultPluginSpec {
        module_name: "gpu_data",
        plugin_name: "gpu-plugin",
        config_message: "GpuConfig",
        result_message: "GpuData",
        config_table: "gpu_config",
        result_table: "gpu_data",
    },
];

#[derive(Clone, Copy, Debug)]
pub(crate) struct FixedResultPluginSpec {
    pub(crate) module_name: &'static str,
    pub(crate) plugin_name: &'static str,
    pub(crate) config_message: &'static str,
    pub(crate) result_message: &'static str,
    pub(crate) config_table: &'static str,
    pub(crate) result_table: &'static str,
}

#[derive(Clone, Debug)]
pub(crate) struct FixedResultChildTableSpec {
    pub(crate) parent_variant: String,
    pub(crate) module_name: String,
    pub(crate) field_name: String,
    pub(crate) child_message: String,
    pub(crate) table_name: String,
}

impl FixedResultPluginSpec {
    pub(crate) fn config_variant(self) -> String {
        self.config_message.to_string()
    }

    pub(crate) fn result_variant(self) -> String {
        self.result_message.to_string()
    }

    pub(crate) fn decoder_type(self) -> String {
        format!(
            "{}PluginDecoder",
            snake_to_upper_camel(&self.plugin_name.replace('-', "_"))
        )
    }

    pub(crate) fn decoder_constructor(self) -> String {
        format!("new_{}_plugin_decoder", self.plugin_name.replace('-', "_"))
    }
}

pub(crate) fn fixed_result_serializable_message_paths(fds: &FileDescriptorSet) -> Vec<String> {
    let mut paths = Vec::new();

    for proto_path in FIXED_RESULT_PROTO_FILES {
        let file = proto_file(fds, proto_path);
        let package = file
            .package
            .as_deref()
            .unwrap_or_else(|| panic!("{proto_path} should declare a package"));

        for message in &file.message_type {
            let message_name = message
                .name
                .as_deref()
                .expect("descriptor message should have a name");
            paths.push(format!(".{package}.{message_name}"));
        }
    }

    paths
}

pub(crate) fn fixed_result_enum_paths(fds: &FileDescriptorSet) -> Vec<String> {
    let mut paths = Vec::new();

    for proto_path in FIXED_RESULT_PROTO_FILES {
        let file = proto_file(fds, proto_path);
        let package = file
            .package
            .as_deref()
            .unwrap_or_else(|| panic!("{proto_path} should declare a package"));

        for enum_type in &file.enum_type {
            let enum_name = enum_type
                .name
                .as_deref()
                .expect("descriptor enum should have a name");
            paths.push(format!(".{package}.{enum_name}"));
        }

        for message in &file.message_type {
            collect_nested_enum_paths(package, "", message, &mut paths);
        }
    }

    paths
}

pub(crate) fn fixed_result_child_table_specs(
    fds: &FileDescriptorSet,
    specs: &[FixedResultPluginSpec],
) -> Vec<FixedResultChildTableSpec> {
    let mut child_tables = Vec::new();

    for spec in specs {
        let result = fixed_result_message_descriptor(fds, spec);
        for field in &result.field {
            if !is_top_level_repeated_message(field) {
                continue;
            }

            let field_name = field
                .name
                .as_deref()
                .expect("descriptor field should have a name");
            let type_name = field
                .type_name
                .as_deref()
                .expect("repeated message field should have a type name");
            let child_message = type_name
                .rsplit('.')
                .next()
                .expect("type name should contain a message name");

            child_tables.push(FixedResultChildTableSpec {
                parent_variant: spec.result_variant(),
                module_name: spec.module_name.to_string(),
                field_name: field_name.to_string(),
                child_message: proto_message_to_rust_type(child_message),
                table_name: format!("{}_{}", spec.result_table, field_name),
            });
        }
    }

    child_tables
}

pub(crate) fn generate_fixed_result_records(
    specs: &[FixedResultPluginSpec],
) -> std::io::Result<()> {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    fs::write(
        out_dir.join(FIXED_RESULT_RECORDS_FILE),
        render_records(specs),
    )
}

fn collect_nested_enum_paths(
    package: &str,
    parent: &str,
    message: &DescriptorProto,
    paths: &mut Vec<String>,
) {
    let message_name = message
        .name
        .as_deref()
        .expect("descriptor message should have a name");
    let full_message_name = if parent.is_empty() {
        format!(".{package}.{message_name}")
    } else {
        format!("{parent}.{message_name}")
    };

    for enum_type in &message.enum_type {
        let enum_name = enum_type
            .name
            .as_deref()
            .expect("descriptor enum should have a name");
        paths.push(format!("{full_message_name}.{enum_name}"));
    }

    for nested in &message.nested_type {
        collect_nested_enum_paths(package, &full_message_name, nested, paths);
    }
}

fn fixed_result_message_descriptor<'a>(
    fds: &'a FileDescriptorSet,
    spec: &FixedResultPluginSpec,
) -> &'a DescriptorProto {
    let package = format!("kat.{}", spec.module_name);

    for proto_path in FIXED_RESULT_PROTO_FILES {
        let file = proto_file(fds, proto_path);
        if file.package.as_deref() != Some(package.as_str()) {
            continue;
        }

        if let Some(message) = file
            .message_type
            .iter()
            .find(|message| message.name.as_deref() == Some(spec.result_message))
        {
            return message;
        }
    }

    panic!(
        "{} should exist in fixed result proto package {}",
        spec.result_message, package
    );
}

fn is_top_level_repeated_message(field: &FieldDescriptorProto) -> bool {
    field.label == Some(Label::Repeated as i32) && field.r#type == Some(Type::Message as i32)
}

fn render_records(specs: &[FixedResultPluginSpec]) -> String {
    let mut output = String::new();
    output.push_str("// @generated by crates/kat-rs-datasource/build.rs. Do not edit.\n\n");
    output.push_str("use anyhow::Result;\n\n");
    output.push_str("use crate::{\n");
    output.push_str("    formats::hitrace::profiler::{\n");
    output.push_str("        PluginDecoder, PluginDecoderSpec, PluginEnvelope, decode_payload,\n");
    output.push_str("    },\n");
    output.push_str("    domains::fixed_result::{FixedResultMessage, ProfilerEnvelopeMeta},\n");
    output.push_str("    proto::kat::{\n");
    for spec in specs {
        writeln!(
            output,
            "        {}::{{{}, {}}},",
            spec.module_name, spec.config_message, spec.result_message
        )
        .expect("write to string");
    }
    output.push_str("    },\n");
    output.push_str("    record::{TraceRecord, TraceRecordSink},\n");
    output.push_str("};\n\n");

    output.push_str("pub(crate) const FIXED_RESULT_PLUGIN_DECODERS: &[PluginDecoderSpec] = &[\n");
    for spec in specs {
        writeln!(
            output,
            "    PluginDecoderSpec::new({}),",
            spec.decoder_constructor()
        )
        .expect("write to string");
    }
    output.push_str("];\n\n");

    output.push_str("#[derive(Clone, Debug)]\n");
    output.push_str("pub(crate) enum FixedResultRecord {\n");
    for spec in specs {
        writeln!(
            output,
            "    {}(Box<FixedResultMessage<{}>>),",
            spec.config_variant(),
            spec.config_message
        )
        .expect("write to string");
        writeln!(
            output,
            "    {}(Box<FixedResultMessage<{}>>),",
            spec.result_variant(),
            spec.result_message
        )
        .expect("write to string");
    }
    output.push_str("}\n\n");

    for spec in specs {
        render_decoder(&mut output, spec);
    }

    output.push_str(
        "fn push_fixed_result_record(\n\
             sink: &mut dyn TraceRecordSink,\n\
             record: FixedResultRecord,\n\
         ) -> Result<()> {\n\
             sink.push(TraceRecord::FixedResult(Box::new(record)))\n\
         }\n",
    );

    output
}

fn render_decoder(output: &mut String, spec: &FixedResultPluginSpec) {
    writeln!(
        output,
        "fn {}() -> Box<dyn PluginDecoder> {{",
        spec.decoder_constructor()
    )
    .expect("write to string");
    writeln!(output, "    Box::new({})", spec.decoder_type()).expect("write to string");
    output.push_str("}\n\n");
    writeln!(output, "struct {};\n\n", spec.decoder_type()).expect("write to string");

    writeln!(output, "impl PluginDecoder for {} {{", spec.decoder_type()).expect("write to string");
    output.push_str("    fn plugin_name(&self) -> &'static str {\n");
    writeln!(output, "        {:?}", spec.plugin_name).expect("write to string");
    output.push_str("    }\n\n");
    output.push_str(
        "    fn configure(\n\
         \x20       &mut self,\n\
         \x20       envelope: &PluginEnvelope<'_>,\n\
         \x20       sink: &mut dyn TraceRecordSink,\n\
         \x20   ) -> Result<()> {\n",
    );
    writeln!(
        output,
        "        let config: {} = decode_payload(envelope)?;",
        spec.config_message
    )
    .expect("write to string");
    writeln!(
        output,
        "        push_fixed_result_record(sink, FixedResultRecord::{}(Box::new(FixedResultMessage::new(ProfilerEnvelopeMeta::from_envelope(envelope), config))))",
        spec.config_variant()
    )
    .expect("write to string");
    output.push_str("    }\n\n");
    output.push_str(
        "    fn decode_data(\n\
         \x20       &mut self,\n\
         \x20       envelope: &PluginEnvelope<'_>,\n\
         \x20       sink: &mut dyn TraceRecordSink,\n\
         \x20   ) -> Result<()> {\n",
    );
    writeln!(
        output,
        "        let result: {} = decode_payload(envelope)?;",
        spec.result_message
    )
    .expect("write to string");
    writeln!(
        output,
        "        push_fixed_result_record(sink, FixedResultRecord::{}(Box::new(FixedResultMessage::new(ProfilerEnvelopeMeta::from_envelope(envelope), result))))",
        spec.result_variant()
    )
    .expect("write to string");
    output.push_str("    }\n");
    output.push_str("}\n\n");
}
