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
    row::{CellValue, ColumnSpec, ColumnType},
    table_batch::TableColumnBuilders,
};

pub(super) type Ordinals = SmallVec<[usize; 4]>;

struct MessageValuePlan {
    message: &'static MessageDescriptor,
    scalar_fields: Vec<ScalarValueField>,
    oneof_groups: Vec<OneofGroupValuePlan>,
}

struct ScalarValueField {
    field: &'static FieldDescriptor,
    column_type: ColumnType,
}

struct OneofGroupValuePlan {
    name: &'static str,
    fields: Vec<&'static FieldDescriptor>,
    field_by_json_key: HashMap<String, &'static str>,
}

static MESSAGE_VALUE_PLANS: OnceLock<HashMap<&'static str, MessageValuePlan>> = OnceLock::new();

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
                    parent_index,
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
                        ordinals: next_ordinals.clone(),
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
    parent_indexes.get(table_name?)?.get(ordinals).copied()
}

pub(super) fn table_columns(message_name: &str) -> Result<Vec<ColumnSpec>> {
    let message = message_descriptor(message_name)?;
    let mut columns = scalar_columns(message)?;
    columns.extend(oneof_group_columns(message)?);
    Ok(columns)
}

fn scalar_columns(message: &MessageDescriptor) -> Result<Vec<ColumnSpec>> {
    Ok(message_value_plan(message.name)?
        .scalar_fields
        .iter()
        .flat_map(|scalar_field| {
            scalar_field_columns(scalar_field.field).expect("scalar field column resolves")
        })
        .collect())
}

fn oneof_group_columns(message: &MessageDescriptor) -> Result<Vec<ColumnSpec>> {
    Ok(message_value_plan(message.name)?
        .oneof_groups
        .iter()
        .map(|group| ColumnSpec::new(group.name, ColumnType::String))
        .collect())
}

pub(super) fn append_table_values(
    builders: &mut TableColumnBuilders,
    value: &PayloadValue,
    message_name: &str,
) -> Result<(usize, usize)> {
    let plan = message_value_plan(message_name)?;
    let mut column_index = 0usize;
    let mut estimated_bytes = 0usize;
    append_scalar_values(
        builders,
        value,
        plan,
        &mut column_index,
        &mut estimated_bytes,
    )?;
    append_oneof_group_values(
        builders,
        value,
        plan,
        &mut column_index,
        &mut estimated_bytes,
    )?;
    Ok((column_index, estimated_bytes))
}

fn append_scalar_values(
    builders: &mut TableColumnBuilders,
    value: &PayloadValue,
    plan: &MessageValuePlan,
    column_index: &mut usize,
    estimated_bytes: &mut usize,
) -> Result<()> {
    for scalar_field in &plan.scalar_fields {
        let field = scalar_field.field;
        let ProtoFieldType::Scalar(scalar_type) = field.field_type else {
            continue;
        };
        let field_value = json_child(value, field.name).unwrap_or(&PayloadValue::Null);
        *estimated_bytes += builders
            .append_payload_value(
                *column_index,
                field.name,
                field_value,
                scalar_field.column_type,
            )
            .with_context(|| {
                format!(
                    "failed to convert field {}.{}",
                    plan.message.name, field.name
                )
            })?;
        *column_index += 1;

        if scalar_type == ProtoScalarType::Enum {
            let enum_column = format!("{}_name", field.name);
            *estimated_bytes += builders.append_cell(
                *column_index,
                &enum_column,
                enum_name_cell(field_value, field),
            )?;
            *column_index += 1;
        }
    }

    Ok(())
}

fn append_oneof_group_values(
    builders: &mut TableColumnBuilders,
    value: &PayloadValue,
    plan: &MessageValuePlan,
    column_index: &mut usize,
    estimated_bytes: &mut usize,
) -> Result<()> {
    for group in &plan.oneof_groups {
        let value = selected_oneof_field_name(value, group);
        *estimated_bytes += builders.append_string_value(*column_index, group.name, value)?;
        *column_index += 1;
    }

    Ok(())
}

pub(super) fn append_value_row_values(
    builders: &mut TableColumnBuilders,
    value: &PayloadValue,
    field: &FieldDescriptor,
) -> Result<(usize, usize)> {
    let ProtoFieldType::Scalar(scalar_type) = field.field_type else {
        bail!("value table field {} is not scalar", field.name);
    };

    let mut column_index = 0usize;
    let mut estimated_bytes = 0usize;
    let column_type = scalar_type_to_column_type(scalar_type)?;
    estimated_bytes += builders.append_payload_value(column_index, "value", value, column_type)?;
    column_index += 1;

    if scalar_type == ProtoScalarType::Enum {
        estimated_bytes +=
            builders.append_cell(column_index, "value_name", enum_name_cell(value, field))?;
        column_index += 1;
    }

    Ok((column_index, estimated_bytes))
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

fn oneof_group_plans(message: &'static MessageDescriptor) -> Vec<OneofGroupValuePlan> {
    let mut groups = Vec::<OneofGroupValuePlan>::new();
    for field in message.fields {
        let Some(name) = field.oneof_name else {
            continue;
        };
        let group_index = if let Some(index) = groups.iter().position(|group| group.name == name) {
            index
        } else {
            let index = groups.len();
            groups.push(OneofGroupValuePlan {
                name,
                fields: Vec::new(),
                field_by_json_key: HashMap::new(),
            });
            index
        };
        let group = &mut groups[group_index];
        group.fields.push(field);
        group
            .field_by_json_key
            .insert(field.name.to_string(), field.name);
        group
            .field_by_json_key
            .insert(snake_to_upper_camel(field.name), field.name);
    }
    groups
}

fn selected_oneof_field_name(
    value: &PayloadValue,
    group: &OneofGroupValuePlan,
) -> Option<&'static str> {
    if let Some((json_key, _)) = oneof_variant_object_value_at(value, group.name) {
        return group.field_by_json_key.get(json_key).copied();
    }

    for field in &group.fields {
        if json_child(value, field.name).is_some() {
            return Some(field.name);
        }
    }

    None
}

fn enum_name_cell(value: &PayloadValue, field: &FieldDescriptor) -> CellValue {
    let Ok(CellValue::I32(number)) = json_to_cell(value, &ColumnType::I32) else {
        return CellValue::Null;
    };
    field
        .enum_values
        .iter()
        .find(|enum_value| enum_value.number == number)
        .map(|enum_value| CellValue::String(enum_value.name.to_string()))
        .unwrap_or(CellValue::Null)
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

fn scalar_field_columns(field: &FieldDescriptor) -> Result<Vec<ColumnSpec>> {
    let ProtoFieldType::Scalar(scalar_type) = field.field_type else {
        return Ok(Vec::new());
    };
    let mut columns = vec![ColumnSpec::new(
        field.name,
        scalar_type_to_column_type(scalar_type)?,
    )];
    if scalar_type == ProtoScalarType::Enum {
        columns.push(ColumnSpec::new(
            format!("{}_name", field.name),
            ColumnType::String,
        ));
    }
    Ok(columns)
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
        columns.push(ColumnSpec::new("value_name", ColumnType::String));
    }
    Ok(columns)
}

fn message_value_plan(message_name: &str) -> Result<&'static MessageValuePlan> {
    MESSAGE_VALUE_PLANS
        .get_or_init(build_message_value_plans)
        .get(message_name)
        .with_context(|| format!("missing message value plan: {message_name}"))
}

fn build_message_value_plans() -> HashMap<&'static str, MessageValuePlan> {
    RELATIONAL_DESCRIPTORS
        .iter()
        .map(|message| (message.name, MessageValuePlan::new(message)))
        .collect()
}

impl MessageValuePlan {
    fn new(message: &'static MessageDescriptor) -> Self {
        Self {
            message,
            scalar_fields: message
                .fields
                .iter()
                .filter_map(|field| match (field.field_type, field.oneof_name) {
                    _ if field.label == ProtoFieldLabel::Repeated => None,
                    (_, Some(_)) => None,
                    (ProtoFieldType::Scalar(scalar_type), None) => Some(ScalarValueField {
                        field,
                        column_type: scalar_type_to_column_type(scalar_type)
                            .expect("proto scalar type maps to column type"),
                    }),
                    (ProtoFieldType::Message(_), None) => None,
                })
                .collect(),
            oneof_groups: oneof_group_plans(message),
        }
    }
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

fn json_to_cell(value: &PayloadValue, column_type: &ColumnType) -> Result<CellValue> {
    if value.is_null() {
        return Ok(CellValue::Null);
    }

    match column_type {
        ColumnType::Binary => payload_bytes(value)
            .map(CellValue::Binary)
            .context("expected binary value"),
        ColumnType::Bool => value
            .as_bool()
            .map(CellValue::Bool)
            .context("expected bool value"),
        ColumnType::I32 => value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .map(CellValue::I32)
            .context("expected i32 value"),
        ColumnType::I64 => value
            .as_i64()
            .map(CellValue::I64)
            .context("expected i64 value"),
        ColumnType::U32 => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(CellValue::U32)
            .context("expected u32 value"),
        ColumnType::U64 => value
            .as_u64()
            .map(CellValue::U64)
            .context("expected u64 value"),
        ColumnType::F32 => value
            .as_f64()
            .map(|value| CellValue::F32(value as f32))
            .context("expected f32 value"),
        ColumnType::F64 => value
            .as_f64()
            .map(CellValue::F64)
            .context("expected f64 value"),
        ColumnType::String => value
            .as_str()
            .map(|value| CellValue::String(value.to_string()))
            .context("expected string value"),
    }
}

fn payload_bytes(value: &PayloadValue) -> Option<Vec<u8>> {
    if let Some(value) = value.as_binary() {
        return Some(value.to_vec());
    }
    value.as_str().map(|value| value.as_bytes().to_vec())
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
