//! 运行时 protobuf value 到 ArrowTable 的通用转换。

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use arrow_array::builder::{
    BinaryBuilder, BooleanBuilder, Float32Builder, Float64Builder, Int32Builder, Int64Builder,
    StringBuilder, UInt32Builder, UInt64Builder,
};
use arrow_array::{ArrayRef, RecordBatch};
use prost_reflect::{DynamicMessage, ReflectMessage, Value};

use crate::{schema_for_table, ArrowTable, ArrowType, FieldSpec, TableSpec};

/// 根据表结构描述，把动态 protobuf message 写入 ArrowTable。
pub fn build_table(
    table: &'static TableSpec,
    records: impl IntoIterator<Item = DynamicMessage>,
) -> Result<Option<ArrowTable>> {
    let records = records.into_iter().collect::<Vec<_>>();
    if records.is_empty() {
        return Ok(None);
    }

    let mut builders = table
        .fields
        .iter()
        .map(|field| ColumnBuilder::new(field.arrow_type))
        .collect::<Vec<_>>();

    for record in records {
        append_record(table, &mut builders, &record)?;
    }

    finish_table(table, builders).map(Some)
}

/// 将一条 protobuf message 追加到所有列 builder。
fn append_record(
    table: &TableSpec,
    builders: &mut [ColumnBuilder],
    record: &DynamicMessage,
) -> Result<()> {
    for (field, builder) in table.fields.iter().zip(builders.iter_mut()) {
        let descriptor = record
            .descriptor()
            .get_field_by_name(field.source)
            .with_context(|| {
                format!(
                    "field `{}` does not exist on protobuf message `{}`",
                    field.source, table.source
                )
            })?;

        if field.nullable && !record.has_field(&descriptor) {
            builder.append_null();
            continue;
        }

        let value = record.get_field(&descriptor);
        match value.as_ref() {
            Value::List(_) => {
                builder.append_value(field, value.as_ref())?;
            }
            other => {
                builder.append_value(field, other)?;
            }
        }
    }

    Ok(())
}

/// 支持的 Arrow 列 builder 集合。
enum ColumnBuilder {
    Boolean(BooleanBuilder),
    Int32(Int32Builder),
    Int64(Int64Builder),
    UInt32(UInt32Builder),
    UInt64(UInt64Builder),
    Float32(Float32Builder),
    Float64(Float64Builder),
    Utf8(StringBuilder),
    Binary(BinaryBuilder),
}

impl ColumnBuilder {
    /// 根据列类型创建对应 Arrow builder。
    fn new(arrow_type: ArrowType) -> Self {
        match arrow_type {
            ArrowType::Boolean => Self::Boolean(BooleanBuilder::new()),
            ArrowType::Int32 => Self::Int32(Int32Builder::new()),
            ArrowType::Int64 => Self::Int64(Int64Builder::new()),
            ArrowType::UInt32 => Self::UInt32(UInt32Builder::new()),
            ArrowType::UInt64 => Self::UInt64(UInt64Builder::new()),
            ArrowType::Float32 => Self::Float32(Float32Builder::new()),
            ArrowType::Float64 => Self::Float64(Float64Builder::new()),
            ArrowType::Utf8 => Self::Utf8(StringBuilder::new()),
            ArrowType::Binary => Self::Binary(BinaryBuilder::new()),
        }
    }

    /// 追加一个 null 值。
    fn append_null(&mut self) {
        match self {
            Self::Boolean(builder) => builder.append_null(),
            Self::Int32(builder) => builder.append_null(),
            Self::Int64(builder) => builder.append_null(),
            Self::UInt32(builder) => builder.append_null(),
            Self::UInt64(builder) => builder.append_null(),
            Self::Float32(builder) => builder.append_null(),
            Self::Float64(builder) => builder.append_null(),
            Self::Utf8(builder) => builder.append_null(),
            Self::Binary(builder) => builder.append_null(),
        }
    }

    /// 按字段描述追加一个 protobuf value。
    fn append_value(&mut self, field: &FieldSpec, value: &Value) -> Result<()> {
        if field.repeated {
            bail!(
                "repeated field `{}` is not supported at runtime yet",
                field.name
            );
        }

        match (self, value) {
            (Self::Boolean(builder), Value::Bool(value)) => builder.append_value(*value),
            (Self::Int32(builder), Value::I32(value)) => builder.append_value(*value),
            (Self::Int32(builder), Value::EnumNumber(value)) => builder.append_value(*value),
            (Self::Int64(builder), Value::I64(value)) => builder.append_value(*value),
            (Self::UInt32(builder), Value::U32(value)) => builder.append_value(*value),
            (Self::UInt64(builder), Value::U64(value)) => builder.append_value(*value),
            (Self::Float32(builder), Value::F32(value)) => builder.append_value(*value),
            (Self::Float64(builder), Value::F64(value)) => builder.append_value(*value),
            (Self::Utf8(builder), Value::String(value)) => builder.append_value(value),
            (Self::Binary(builder), Value::Bytes(value)) => builder.append_value(value.as_ref()),
            (_, other) => {
                bail!(
                    "field `{}` cannot append protobuf value {:?} as {:?}",
                    field.name,
                    other,
                    field.arrow_type
                );
            }
        }
        Ok(())
    }

    /// 完成 builder 并返回 ArrayRef。
    fn finish(&mut self) -> ArrayRef {
        match self {
            Self::Boolean(builder) => Arc::new(builder.finish()),
            Self::Int32(builder) => Arc::new(builder.finish()),
            Self::Int64(builder) => Arc::new(builder.finish()),
            Self::UInt32(builder) => Arc::new(builder.finish()),
            Self::UInt64(builder) => Arc::new(builder.finish()),
            Self::Float32(builder) => Arc::new(builder.finish()),
            Self::Float64(builder) => Arc::new(builder.finish()),
            Self::Utf8(builder) => Arc::new(builder.finish()),
            Self::Binary(builder) => Arc::new(builder.finish()),
        }
    }
}

/// 将列 builders 完成成 ArrowTable。
fn finish_table(table: &'static TableSpec, mut builders: Vec<ColumnBuilder>) -> Result<ArrowTable> {
    let schema = schema_for_table(table);
    let columns = builders
        .iter_mut()
        .map(ColumnBuilder::finish)
        .collect::<Vec<_>>();
    let batch = RecordBatch::try_new(schema.clone(), columns)
        .with_context(|| format!("failed to build RecordBatch for table `{}`", table.name))?;

    ArrowTable::new(table.name, schema, vec![batch])
}
