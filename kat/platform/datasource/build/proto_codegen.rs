use prost_types::{DescriptorProto, FileDescriptorSet, field_descriptor_proto::Type};

#[derive(Clone, Debug)]
pub(crate) struct ProtoMessage {
    pub(crate) name: String,
    pub(crate) table_name: String,
}

pub(crate) fn bytes_field_paths(fds: &FileDescriptorSet) -> Vec<String> {
    let mut paths = Vec::new();
    for file in &fds.file {
        let package = file.package.as_deref().unwrap_or("");
        for message in &file.message_type {
            collect_bytes_field_paths(package, "", message, &mut paths);
        }
    }
    paths
}

fn collect_bytes_field_paths(
    package: &str,
    parent_path: &str,
    message: &DescriptorProto,
    paths: &mut Vec<String>,
) {
    let message_name = message
        .name
        .as_deref()
        .expect("descriptor message should have a name");
    let message_path = if parent_path.is_empty() {
        message_name.to_string()
    } else {
        format!("{parent_path}.{message_name}")
    };
    let qualified_message = if package.is_empty() {
        message_path.clone()
    } else {
        format!("{package}.{message_path}")
    };

    for field in &message.field {
        if field.r#type == Some(Type::Bytes as i32) {
            let field_name = field
                .name
                .as_deref()
                .expect("descriptor field should have a name");
            paths.push(format!(".{qualified_message}.{field_name}"));
        }
    }

    for nested in &message.nested_type {
        collect_bytes_field_paths(package, &message_path, nested, paths);
    }
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
    let mut camel = String::new();
    for part in name.split('_').filter(|part| !part.is_empty()) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            camel.push(first.to_ascii_uppercase());
            camel.push_str(chars.as_str());
        }
    }
    camel
}

pub(crate) fn camel_to_snake(name: &str) -> String {
    let mut snake = String::new();
    for (index, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index != 0 {
                snake.push('_');
            }
            snake.push(ch.to_ascii_lowercase());
        } else {
            snake.push(ch);
        }
    }
    snake
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
