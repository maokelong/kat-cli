use std::collections::HashSet;

use super::{
    descriptor::{MessageDescriptor, ProtoFieldLabel, ProtoFieldType, RELATIONAL_DESCRIPTORS},
    rules::ExpansionRule,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExpansionPlanItem {
    pub(crate) rule: ExpansionRule,
    pub(crate) root_message: String,
    pub(crate) source_path: Vec<String>,
    pub(crate) source_message: String,
    pub(crate) output_table: String,
}

pub(crate) fn expansion_plan_for_roots(root_messages: &[&str]) -> Vec<ExpansionPlanItem> {
    let mut items = Vec::new();

    for root_message in root_messages {
        let Some(message) = message_descriptor(root_message) else {
            continue;
        };

        add_root_scalars(&mut items, root_message, message);

        let mut visited = HashSet::new();
        collect_nested_tables(&mut items, root_message, message, Vec::new(), &mut visited);
    }

    items
}

fn add_root_scalars(
    items: &mut Vec<ExpansionPlanItem>,
    root_message: &str,
    message: &MessageDescriptor,
) {
    if !message.fields.iter().any(|field| {
        field.label != ProtoFieldLabel::Repeated
            && field.oneof_name.is_none()
            && matches!(field.field_type, ProtoFieldType::Scalar(_))
    }) {
        return;
    }

    items.push(ExpansionPlanItem {
        rule: ExpansionRule::RootScalars,
        root_message: root_message.to_string(),
        source_path: Vec::new(),
        source_message: root_message.to_string(),
        output_table: table_name(root_message, &[]),
    });
}

fn collect_nested_tables(
    items: &mut Vec<ExpansionPlanItem>,
    root_message: &str,
    message: &MessageDescriptor,
    current_path: Vec<String>,
    visited: &mut HashSet<(String, String)>,
) {
    let visit_key = (message.name.to_string(), current_path.join("."));
    if !visited.insert(visit_key) {
        return;
    }

    for field in message.fields {
        if field.label == ProtoFieldLabel::Repeated {
            let source_path = append_segment(&current_path, field.name);
            let ProtoFieldType::Message(source_message) = field.field_type else {
                items.push(ExpansionPlanItem {
                    rule: ExpansionRule::RepeatedScalar,
                    root_message: root_message.to_string(),
                    source_path,
                    source_message: message.name.to_string(),
                    output_table: table_name(
                        root_message,
                        &append_segment(&current_path, field.name),
                    ),
                });
                continue;
            };

            let Some(child_message) = message_descriptor(source_message) else {
                continue;
            };

            let source_path = append_segment(&current_path, field.name);
            items.push(ExpansionPlanItem {
                rule: ExpansionRule::RepeatedMessage,
                root_message: root_message.to_string(),
                source_path: source_path.clone(),
                source_message: source_message.to_string(),
                output_table: table_name(root_message, &source_path),
            });

            collect_nested_tables(items, root_message, child_message, source_path, visited);
            continue;
        }

        if let Some(oneof_name) = field.oneof_name {
            let source_path = append_segments(&current_path, &[oneof_name, field.name]);
            items.push(ExpansionPlanItem {
                rule: ExpansionRule::OneofVariantTable,
                root_message: root_message.to_string(),
                source_path: source_path.clone(),
                source_message: message.name.to_string(),
                output_table: table_name(root_message, &source_path),
            });

            if let ProtoFieldType::Message(source_message) = field.field_type
                && let Some(child_message) = message_descriptor(source_message)
            {
                collect_nested_tables(items, root_message, child_message, source_path, visited);
            }
            continue;
        }

        let ProtoFieldType::Message(source_message) = field.field_type else {
            continue;
        };

        let source_path = append_segment(&current_path, field.name);
        items.push(ExpansionPlanItem {
            rule: ExpansionRule::MessageFieldTable,
            root_message: root_message.to_string(),
            source_path: source_path.clone(),
            source_message: source_message.to_string(),
            output_table: table_name(root_message, &source_path),
        });

        if let Some(child_message) = message_descriptor(source_message) {
            collect_nested_tables(items, root_message, child_message, source_path, visited);
        }
    }
}

fn append_segment(path: &[String], segment: &str) -> Vec<String> {
    let mut output = path.to_vec();
    output.push(segment.to_string());
    output
}

fn append_segments(path: &[String], segments: &[&str]) -> Vec<String> {
    let mut output = path.to_vec();
    output.extend(segments.iter().map(|segment| (*segment).to_string()));
    output
}

fn message_descriptor(name: &str) -> Option<&'static MessageDescriptor> {
    RELATIONAL_DESCRIPTORS
        .iter()
        .find(|message| message.name == name)
}

pub(crate) fn table_name(root_message: &str, path: &[String]) -> String {
    let mut name = camel_to_snake(root_message);
    let mut message = message_descriptor(root_message);
    for segment in path {
        if message.is_some_and(|message| is_oneof_group(message, segment)) {
            continue;
        }

        name.push_str("__");
        name.push_str(segment);

        if let Some(current_message) = message {
            message = current_message
                .fields
                .iter()
                .find(|field| field.name == segment)
                .and_then(|field| match field.field_type {
                    ProtoFieldType::Message(next_message) => message_descriptor(next_message),
                    ProtoFieldType::Scalar(_) => None,
                });
        }
    }
    name
}

fn is_oneof_group(message: &MessageDescriptor, segment: &str) -> bool {
    message
        .fields
        .iter()
        .any(|field| field.oneof_name == Some(segment))
}

fn camel_to_snake(name: &str) -> String {
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
