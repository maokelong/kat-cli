use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

use anyhow::{Context, Result, bail};
use smallvec::SmallVec;

use crate::{payload_value::PayloadValue, record::DecodedPayload};

use super::{
    descriptor::{
        FieldDescriptor, MessageDescriptor, ProtoFieldLabel, ProtoFieldType, ProtoScalarType,
        RELATIONAL_DESCRIPTORS,
    },
    plan::ExpansionPlanItem,
    row::{ColumnProjection, ColumnSpec, ColumnType, OneofVariantName},
    table_batch::TableColumnBuilders,
};

pub(super) type Ordinals = SmallVec<[usize; 4]>;

struct MessageColumnPlan {
    columns: Vec<ColumnSpec>,
}

static MESSAGE_COLUMN_PLANS: OnceLock<HashMap<&'static str, MessageColumnPlan>> = OnceLock::new();

pub(super) struct RowSource<'a> {
    pub(super) value: &'a PayloadValue,
    pub(super) parent_index: Option<u64>,
    pub(super) ordinals: Ordinals,
}

pub(super) fn collect_present_child_fields_at_path(
    value: &PayloadValue,
    path: &[String],
) -> HashSet<String> {
    let mut fields = HashSet::new();
    collect_present_child_fields(value, path, 0, &mut fields);
    fields
}

fn collect_present_child_fields(
    value: &PayloadValue,
    path: &[String],
    path_index: usize,
    fields: &mut HashSet<String>,
) {
    let Some(segment) = path.get(path_index) else {
        let Some(object) = value.as_object() else {
            return;
        };
        for field in object {
            if field.value().is_null() {
                continue;
            }
            fields.insert(field.name().to_string());
            fields.insert(upper_camel_to_snake(field.name()));
        }
        return;
    };

    let Some(child) = json_child(value, segment) else {
        return;
    };
    if let Some(values) = child.as_array() {
        for value in values {
            collect_present_child_fields(value, path, path_index + 1, fields);
        }
    } else if child.is_object() {
        collect_present_child_fields(child, path, path_index + 1, fields);
    }
}

pub(super) fn visit_row_sources_at_path<'a, F>(
    payload: &'a DecodedPayload,
    source_path: &[String],
    parent_table_by_segment: &[Option<String>],
    parent_indexes: &HashMap<String, HashMap<Ordinals, u64>>,
    visitor: &mut F,
) -> Result<()>
where
    F: FnMut(RowSource<'a>) -> Result<()>,
{
    visit_row_sources(
        source_path,
        parent_table_by_segment,
        parent_indexes,
        &payload.message,
        0,
        &Ordinals::new(),
        None,
        visitor,
    )
}

#[allow(clippy::too_many_arguments)]
fn visit_row_sources<'a, F>(
    source_path: &[String],
    parent_table_by_segment: &[Option<String>],
    parent_indexes: &HashMap<String, HashMap<Ordinals, u64>>,
    current: &'a PayloadValue,
    segment_index: usize,
    ordinals: &Ordinals,
    parent_index: Option<u64>,
    visitor: &mut F,
) -> Result<()>
where
    F: FnMut(RowSource<'a>) -> Result<()>,
{
    let Some(segment) = source_path.get(segment_index) else {
        visitor(RowSource {
            value: current,
            parent_index,
            ordinals: ordinals.clone(),
        })?;
        return Ok(());
    };

    let is_final_segment = segment_index + 1 == source_path.len();
    let parent_table = parent_table_by_segment
        .get(segment_index)
        .and_then(|table| table.as_deref());

    match child_at_segment(current, segment)? {
        SegmentValue::Missing => Ok(()),
        SegmentValue::Value(value) => {
            if is_final_segment {
                visitor(RowSource {
                    value,
                    parent_index: parent_index_for_table(parent_indexes, parent_table, ordinals)
                        .or(parent_index),
                    ordinals: ordinals.clone(),
                })?;
                return Ok(());
            }

            bail!("path segment {segment} resolved to a scalar before path end")
        }
        SegmentValue::Object(value) => {
            let next_parent_index =
                parent_index_for_table(parent_indexes, parent_table, ordinals).or(parent_index);

            if is_final_segment {
                visitor(RowSource {
                    value,
                    parent_index: next_parent_index,
                    ordinals: ordinals.clone(),
                })?;
                return Ok(());
            }

            visit_row_sources(
                source_path,
                parent_table_by_segment,
                parent_indexes,
                value,
                segment_index + 1,
                ordinals,
                next_parent_index,
                visitor,
            )
        }
        SegmentValue::Array(values) => {
            for (ordinal, value) in values.iter().enumerate() {
                let mut next_ordinals = ordinals.clone();
                next_ordinals.push(ordinal);
                let next_parent_index =
                    parent_index_for_table(parent_indexes, parent_table, &next_ordinals)
                        .or(parent_index);

                if is_final_segment {
                    visitor(RowSource {
                        value,
                        parent_index: next_parent_index,
                        ordinals: next_ordinals,
                    })?;
                    continue;
                }

                visit_row_sources(
                    source_path,
                    parent_table_by_segment,
                    parent_indexes,
                    value,
                    segment_index + 1,
                    &next_ordinals,
                    next_parent_index,
                    visitor,
                )?;
            }
            Ok(())
        }
    }
}

enum SegmentValue<'a> {
    Missing,
    Value(&'a PayloadValue),
    Object(&'a PayloadValue),
    Array(&'a [PayloadValue]),
}

fn child_at_segment<'a>(current: &'a PayloadValue, segment: &str) -> Result<SegmentValue<'a>> {
    let Some(value) = json_child(current, segment) else {
        return Ok(SegmentValue::Missing);
    };
    if value.is_null() {
        return Ok(SegmentValue::Missing);
    }
    if let Some(values) = value.as_array() {
        return Ok(SegmentValue::Array(values));
    }
    if value.is_object() {
        return Ok(SegmentValue::Object(value));
    }

    Ok(SegmentValue::Value(value))
}

fn parent_index_for_table(
    parent_indexes: &HashMap<String, HashMap<Ordinals, u64>>,
    table_name: Option<&str>,
    ordinals: &Ordinals,
) -> Option<u64> {
    let indexes = parent_indexes.get(table_name?)?;
    for length in (0..=ordinals.len()).rev() {
        let key = Ordinals::from_slice(&ordinals[..length]);
        if let Some(index) = indexes.get(&key) {
            return Some(*index);
        }
    }
    None
}

pub(super) fn table_columns(message_name: &str) -> Result<Vec<ColumnSpec>> {
    Ok(message_column_plan(message_name)?.columns.clone())
}

pub(super) fn append_table_values(
    builders: &mut TableColumnBuilders,
    value: &PayloadValue,
    message_name: &str,
) -> Result<(usize, usize)> {
    let plan = message_column_plan(message_name)?;
    let mut estimated_bytes = 0usize;

    for (column_index, column) in plan.columns.iter().enumerate() {
        let field_value = json_child(value, &column.source_name).unwrap_or(&PayloadValue::Null);
        estimated_bytes += builders
            .append_payload_value(column_index, column, field_value)
            .with_context(|| {
                format!(
                    "failed to convert field {}.{}",
                    message_name, column.source_name
                )
            })?;
    }

    Ok((plan.columns.len(), estimated_bytes))
}

pub(super) fn append_value_row_values(
    builders: &mut TableColumnBuilders,
    value: &PayloadValue,
    field: &FieldDescriptor,
) -> Result<(usize, usize)> {
    let columns = value_columns(field)?;
    let mut estimated_bytes = 0usize;
    for (column_index, column) in columns.iter().enumerate() {
        estimated_bytes += builders.append_payload_value(column_index, column, value)?;
    }
    Ok((columns.len(), estimated_bytes))
}

pub(super) fn oneof_variant_object_value_at<'a>(
    value: &'a PayloadValue,
    oneof_name: &str,
) -> Option<(&'a str, &'a PayloadValue)> {
    let oneof_value = json_child(value, oneof_name)?.as_object()?;
    oneof_value
        .iter()
        .next()
        .map(|field| (field.name(), field.value()))
}

pub(super) fn serde_oneof_variant_key(field_name: &str) -> String {
    snake_to_upper_camel(field_name)
}

pub(super) fn json_child<'a>(
    value: &'a PayloadValue,
    field_name: &str,
) -> Option<&'a PayloadValue> {
    if let Some(value) = payload_child(value, field_name) {
        return Some(value);
    }

    let snake_case = upper_camel_to_snake(field_name);
    if snake_case != field_name
        && let Some(value) = payload_child(value, snake_case.as_str())
    {
        return Some(value);
    }

    let upper_camel = snake_to_upper_camel(field_name);
    payload_child(value, upper_camel.as_str())
}

fn payload_child<'a>(value: &'a PayloadValue, field_name: &str) -> Option<&'a PayloadValue> {
    value
        .as_object()?
        .iter()
        .find(|field| field.name() == field_name)
        .map(|field| field.value())
}

pub(super) fn leaf_field_descriptor(
    message_name: &str,
    item: &ExpansionPlanItem,
) -> Result<&'static FieldDescriptor> {
    let field_name = item
        .source_path
        .last()
        .with_context(|| format!("plan item {} has no source path", item.output_table))?;
    let message = message_descriptor(message_name)?;
    field_descriptor(message, field_name)
}

fn field_descriptor(
    message: &'static MessageDescriptor,
    field_name: &str,
) -> Result<&'static FieldDescriptor> {
    message
        .fields
        .iter()
        .find(|field| field.name == field_name)
        .with_context(|| format!("missing field {}.{}", message.name, field_name))
}

pub(super) fn value_columns(field: &FieldDescriptor) -> Result<Vec<ColumnSpec>> {
    let ProtoFieldType::Scalar(scalar_type) = field.field_type else {
        bail!("value table field {} is not scalar", field.name);
    };
    let mut columns = vec![ColumnSpec::new(
        "value",
        scalar_type_to_column_type(scalar_type)?,
    )];
    if scalar_type == ProtoScalarType::Enum {
        columns.push(ColumnSpec::projected(
            "value_name",
            "value",
            ColumnType::String,
            ColumnProjection::EnumName(field.enum_values),
        ));
    }
    Ok(columns)
}

fn message_column_plan(message_name: &str) -> Result<&'static MessageColumnPlan> {
    MESSAGE_COLUMN_PLANS
        .get_or_init(build_message_column_plans)
        .get(message_name)
        .with_context(|| format!("missing message column plan: {message_name}"))
}

fn build_message_column_plans() -> HashMap<&'static str, MessageColumnPlan> {
    RELATIONAL_DESCRIPTORS
        .iter()
        .map(|message| {
            let mut stack = Vec::new();
            let columns = columns_for_message(message, &mut stack);
            (message.name, MessageColumnPlan { columns })
        })
        .collect()
}

fn columns_for_message(
    message: &'static MessageDescriptor,
    stack: &mut Vec<&'static str>,
) -> Vec<ColumnSpec> {
    assert!(
        !stack.contains(&message.name),
        "recursive singular message cannot be represented as an Arrow Struct: {}",
        message.name
    );
    stack.push(message.name);

    let mut columns = Vec::new();
    let mut oneof_groups = HashSet::new();
    for field in message.fields {
        if let Some(oneof_name) = field.oneof_name {
            if oneof_groups.insert(oneof_name) {
                columns.push(ColumnSpec::projected(
                    oneof_name,
                    oneof_name,
                    ColumnType::String,
                    ColumnProjection::OneofName(oneof_variant_names(message, oneof_name)),
                ));
            }
            continue;
        }

        match (field.label, field.field_type) {
            (ProtoFieldLabel::Repeated, _) => {}
            (ProtoFieldLabel::Optional, ProtoFieldType::Message(message_name)) => {
                let child = message_descriptor(message_name)
                    .expect("message field descriptor is part of relational summary");
                let child_columns = columns_for_message(child, stack);
                if !child_columns.is_empty() {
                    columns.push(ColumnSpec::new(
                        field.name,
                        ColumnType::Struct(child_columns),
                    ));
                }
            }
            (ProtoFieldLabel::Optional, ProtoFieldType::Scalar(ProtoScalarType::Enum)) => {
                columns.push(ColumnSpec::new(field.name, ColumnType::I32));
                columns.push(ColumnSpec::projected(
                    format!("{}_name", field.name),
                    field.name,
                    ColumnType::String,
                    ColumnProjection::EnumName(field.enum_values),
                ));
            }
            (ProtoFieldLabel::Optional, ProtoFieldType::Scalar(scalar_type)) => {
                columns.push(ColumnSpec::new(
                    field.name,
                    scalar_type_to_column_type(scalar_type)
                        .expect("proto scalar maps to Arrow column"),
                ));
            }
        }
    }

    stack.pop();
    columns
}

fn oneof_variant_names(message: &MessageDescriptor, oneof_name: &str) -> Vec<OneofVariantName> {
    message
        .fields
        .iter()
        .filter(|field| field.oneof_name == Some(oneof_name))
        .map(|field| OneofVariantName {
            field_name: field.name,
            serialized_name: snake_to_upper_camel(field.name),
        })
        .collect()
}

fn scalar_type_to_column_type(scalar_type: ProtoScalarType) -> Result<ColumnType> {
    match scalar_type {
        ProtoScalarType::Bytes => Ok(ColumnType::Binary),
        ProtoScalarType::Bool => Ok(ColumnType::Bool),
        ProtoScalarType::I32 | ProtoScalarType::Enum => Ok(ColumnType::I32),
        ProtoScalarType::I64 => Ok(ColumnType::I64),
        ProtoScalarType::U32 => Ok(ColumnType::U32),
        ProtoScalarType::U64 => Ok(ColumnType::U64),
        ProtoScalarType::F32 => Ok(ColumnType::F32),
        ProtoScalarType::F64 => Ok(ColumnType::F64),
        ProtoScalarType::String => Ok(ColumnType::String),
    }
}

fn message_descriptor(name: &str) -> Result<&'static MessageDescriptor> {
    RELATIONAL_DESCRIPTORS
        .iter()
        .find(|message| message.name == name)
        .with_context(|| format!("missing relational message descriptor: {name}"))
}

fn snake_to_upper_camel(value: &str) -> String {
    let mut output = String::new();
    for part in value.split('_').filter(|part| !part.is_empty()) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            output.push(first.to_ascii_uppercase());
            output.extend(chars);
        }
    }
    output
}

fn upper_camel_to_snake(value: &str) -> String {
    let mut output = String::new();
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index != 0 {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push(ch);
        }
    }
    output
}
