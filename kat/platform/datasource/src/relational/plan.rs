use std::collections::HashSet;

use anyhow::{Context, Result};
use heck::ToSnakeCase;

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
    pub(crate) parent_table: Option<String>,
}

pub(crate) fn expansion_plan_for_roots(root_messages: &[&str]) -> Result<Vec<ExpansionPlanItem>> {
    let mut items = Vec::new();

    for root_message in root_messages {
        let message = message_descriptor(root_message).with_context(|| {
            format!("missing relational root message descriptor: {root_message}")
        })?;
        validate_message_descriptors(message, &mut HashSet::new())?;

        let root_table = table_name(root_message, &[]);
        items.push(ExpansionPlanItem {
            rule: ExpansionRule::RootRecord,
            root_message: root_message.to_string(),
            source_path: Vec::new(),
            source_message: root_message.to_string(),
            output_table: root_table.clone(),
            parent_table: None,
        });

        let mut visited = HashSet::new();
        collect_nested_tables(
            &mut items,
            root_message,
            message,
            Vec::new(),
            &root_table,
            &mut visited,
        );
    }

    Ok(items)
}

fn collect_nested_tables(
    items: &mut Vec<ExpansionPlanItem>,
    root_message: &str,
    message: &MessageDescriptor,
    current_path: Vec<String>,
    parent_table: &str,
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
                    parent_table: Some(parent_table.to_string()),
                });
                continue;
            };

            let Some(child_message) = message_descriptor(source_message) else {
                continue;
            };

            let output_table = table_name(root_message, &source_path);
            items.push(ExpansionPlanItem {
                rule: ExpansionRule::RepeatedMessage,
                root_message: root_message.to_string(),
                source_path: source_path.clone(),
                source_message: source_message.to_string(),
                output_table: output_table.clone(),
                parent_table: Some(parent_table.to_string()),
            });

            collect_nested_tables(
                items,
                root_message,
                child_message,
                source_path,
                &output_table,
                visited,
            );
            continue;
        }

        if let Some(oneof_name) = field.oneof_name {
            let source_path = append_segments(&current_path, &[oneof_name, field.name]);
            let output_table = table_name(root_message, &source_path);
            items.push(ExpansionPlanItem {
                rule: ExpansionRule::OneofVariant,
                root_message: root_message.to_string(),
                source_path: source_path.clone(),
                source_message: message.name.to_string(),
                output_table: output_table.clone(),
                parent_table: Some(parent_table.to_string()),
            });

            if let ProtoFieldType::Message(source_message) = field.field_type
                && let Some(child_message) = message_descriptor(source_message)
            {
                collect_nested_tables(
                    items,
                    root_message,
                    child_message,
                    source_path,
                    &output_table,
                    visited,
                );
            }
            continue;
        }

        let ProtoFieldType::Message(source_message) = field.field_type else {
            continue;
        };

        let source_path = append_segment(&current_path, field.name);
        if let Some(child_message) = message_descriptor(source_message) {
            collect_nested_tables(
                items,
                root_message,
                child_message,
                source_path,
                parent_table,
                visited,
            );
        }
    }
}

fn validate_message_descriptors(
    message: &'static MessageDescriptor,
    visited: &mut HashSet<&'static str>,
) -> Result<()> {
    if !visited.insert(message.name) {
        return Ok(());
    }

    for field in message.fields {
        let ProtoFieldType::Message(child_name) = field.field_type else {
            continue;
        };
        let child = message_descriptor(child_name).with_context(|| {
            format!(
                "missing relational message descriptor: {child_name}, referenced by {}.{}",
                message.name, field.name
            )
        })?;
        validate_message_descriptors(child, visited)?;
    }

    Ok(())
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
    let mut name = root_message.to_snake_case();
    let mut message = message_descriptor(root_message);
    for segment in path {
        if message.is_some_and(|message| is_oneof_group(message, segment)) {
            continue;
        }

        name.push('_');
        name.push_str(&segment.to_snake_case());

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
