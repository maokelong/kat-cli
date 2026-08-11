use heck::{ToSnakeCase, ToUpperCamelCase};
use prost_reflect::{DescriptorPool, Kind};
use prost_types::{DescriptorProto, FileDescriptorSet};

#[derive(Clone, Debug)]
pub(crate) struct ProtoMessage {
    pub(crate) name: String,
    pub(crate) table_name: String,
}

pub(crate) fn bytes_field_paths(pool: &DescriptorPool) -> Vec<String> {
    pool.all_messages()
        .flat_map(|message| message.fields().collect::<Vec<_>>())
        .filter(|field| matches!(field.kind(), Kind::Bytes))
        .map(|field| format!(".{}", field.full_name()))
        .collect()
}

pub(crate) fn messages_in_file(fds: &FileDescriptorSet, proto_path: &str) -> Vec<ProtoMessage> {
    proto_file(fds, proto_path)
        .message_type
        .iter()
        .map(|message| {
            let name = message
                .name
                .as_ref()
                .expect("descriptor message should have a name")
                .clone();
            let base_name = name.strip_suffix("Format").unwrap_or(&name).to_string();
            ProtoMessage {
                name,
                table_name: camel_to_snake(&base_name),
            }
        })
        .collect()
}

pub(crate) fn message_in_file<'a>(
    fds: &'a FileDescriptorSet,
    proto_path: &str,
    message_name: &str,
) -> &'a DescriptorProto {
    proto_file(fds, proto_path)
        .message_type
        .iter()
        .find(|message| message.name.as_deref() == Some(message_name))
        .unwrap_or_else(|| panic!("{message_name} should exist in {proto_path}"))
}

pub(crate) fn message_type_name(type_name: &str) -> String {
    type_name
        .rsplit('.')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(type_name)
        .to_string()
}

pub(crate) fn snake_to_upper_camel(name: &str) -> String {
    name.to_upper_camel_case()
}

pub(crate) fn camel_to_snake(name: &str) -> String {
    name.to_snake_case()
}

fn proto_file<'a>(
    fds: &'a FileDescriptorSet,
    proto_path: &str,
) -> &'a prost_types::FileDescriptorProto {
    fds.file
        .iter()
        .find(|file| {
            file.name
                .as_deref()
                .is_some_and(|name| proto_file_name_matches(name, proto_path))
        })
        .unwrap_or_else(|| panic!("{proto_path} should exist in proto descriptor set"))
}

fn proto_file_name_matches(descriptor_name: &str, proto_path: &str) -> bool {
    descriptor_name == proto_path
        || proto_path
            .strip_prefix("proto/")
            .is_some_and(|path_without_include_root| descriptor_name == path_without_include_root)
}
