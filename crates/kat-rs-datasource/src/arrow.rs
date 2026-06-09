//! Converts decoded protobuf messages to Arrow batches using runtime descriptors.

use std::{borrow::Cow, sync::Arc};

use anyhow::{Result, bail};
use arrow_array::{
    ArrayRef, RecordBatch,
    builder::{
        BinaryBuilder, BooleanBuilder, Float32Builder, Float64Builder, Int32Builder, Int64Builder,
        StringBuilder, UInt32Builder, UInt64Builder,
    },
};
use arrow_schema::{DataType, Field, Schema};
use prost_reflect::{DynamicMessage, FieldDescriptor, Kind, MessageDescriptor, Value};

pub(crate) fn save_to_arrow_batch(
    descriptor: MessageDescriptor,
    messages: impl IntoIterator<Item = DynamicMessage>,
) -> Result<RecordBatch> {
    let mut writer = DescriptorArrowBatchWriter::new(descriptor, 0)?;
    for message in messages {
        writer.append(&message)?;
    }
    writer.finish()
}

struct DescriptorArrowBatchWriter {
    schema: Arc<Schema>,
    fields: Vec<FieldDescriptor>,
    columns: Vec<DescriptorColumnWriter>,
}

impl DescriptorArrowBatchWriter {
    fn new(descriptor: MessageDescriptor, capacity: usize) -> Result<Self> {
        let fields = descriptor.fields().collect::<Vec<_>>();
        let schema = Arc::new(Schema::new(
            fields
                .iter()
                .map(DescriptorColumnWriter::field_schema)
                .collect::<Result<Vec<_>>>()?,
        ));
        let columns = fields
            .iter()
            .map(|field| DescriptorColumnWriter::new(field, capacity))
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            schema,
            fields,
            columns,
        })
    }

    fn append(&mut self, message: &DynamicMessage) -> Result<()> {
        for (field, column) in self.fields.iter().zip(self.columns.iter_mut()) {
            let value = message.get_field(field);
            column.append(field, value)?;
        }

        Ok(())
    }

    fn finish(self) -> Result<RecordBatch> {
        let arrays = self
            .columns
            .into_iter()
            .map(DescriptorColumnWriter::finish)
            .collect::<Vec<_>>();

        Ok(RecordBatch::try_new(self.schema, arrays)?)
    }
}

enum DescriptorColumnWriter {
    Binary(BinaryBuilder),
    Bool(BooleanBuilder),
    F32(Float32Builder),
    F64(Float64Builder),
    I32(Int32Builder),
    I64(Int64Builder),
    String(StringBuilder),
    U32(UInt32Builder),
    U64(UInt64Builder),
}

impl DescriptorColumnWriter {
    fn new(field: &FieldDescriptor, capacity: usize) -> Result<Self> {
        ensure_scalar_field(field)?;

        Ok(match field.kind() {
            Kind::Bytes => Self::Binary(BinaryBuilder::new()),
            Kind::Bool => Self::Bool(BooleanBuilder::with_capacity(capacity)),
            Kind::Double => Self::F64(Float64Builder::with_capacity(capacity)),
            Kind::Float => Self::F32(Float32Builder::with_capacity(capacity)),
            Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => {
                Self::I32(Int32Builder::with_capacity(capacity))
            }
            Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => {
                Self::I64(Int64Builder::with_capacity(capacity))
            }
            Kind::String => Self::String(StringBuilder::new()),
            Kind::Uint32 | Kind::Fixed32 => Self::U32(UInt32Builder::with_capacity(capacity)),
            Kind::Uint64 | Kind::Fixed64 => Self::U64(UInt64Builder::with_capacity(capacity)),
            Kind::Enum(_) => Self::I32(Int32Builder::with_capacity(capacity)),
            other => bail!(
                "unsupported protobuf field type for {}: {other:?}",
                field.full_name()
            ),
        })
    }

    fn field_schema(field: &FieldDescriptor) -> Result<Field> {
        ensure_scalar_field(field)?;

        let data_type = match field.kind() {
            Kind::Bytes => DataType::Binary,
            Kind::Bool => DataType::Boolean,
            Kind::Double => DataType::Float64,
            Kind::Float => DataType::Float32,
            Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 | Kind::Enum(_) => DataType::Int32,
            Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => DataType::Int64,
            Kind::String => DataType::Utf8,
            Kind::Uint32 | Kind::Fixed32 => DataType::UInt32,
            Kind::Uint64 | Kind::Fixed64 => DataType::UInt64,
            other => bail!(
                "unsupported protobuf field type for {}: {other:?}",
                field.full_name()
            ),
        };

        Ok(Field::new(field.name(), data_type, false))
    }

    fn append(&mut self, field: &FieldDescriptor, value: Cow<'_, Value>) -> Result<()> {
        match self {
            Self::Binary(builder) => match value.as_ref() {
                Value::Bytes(value) => builder.append_value(value.as_ref()),
                other => type_mismatch(field, other)?,
            },
            Self::Bool(builder) => match value.as_ref() {
                Value::Bool(value) => builder.append_value(*value),
                other => type_mismatch(field, other)?,
            },
            Self::F32(builder) => match value.as_ref() {
                Value::F32(value) => builder.append_value(*value),
                other => type_mismatch(field, other)?,
            },
            Self::F64(builder) => match value.as_ref() {
                Value::F64(value) => builder.append_value(*value),
                other => type_mismatch(field, other)?,
            },
            Self::I32(builder) => match value.as_ref() {
                Value::I32(value) | Value::EnumNumber(value) => builder.append_value(*value),
                other => type_mismatch(field, other)?,
            },
            Self::I64(builder) => match value.as_ref() {
                Value::I64(value) => builder.append_value(*value),
                other => type_mismatch(field, other)?,
            },
            Self::String(builder) => match value.as_ref() {
                Value::String(value) => builder.append_value(value),
                other => type_mismatch(field, other)?,
            },
            Self::U32(builder) => match value.as_ref() {
                Value::U32(value) => builder.append_value(*value),
                other => type_mismatch(field, other)?,
            },
            Self::U64(builder) => match value.as_ref() {
                Value::U64(value) => builder.append_value(*value),
                other => type_mismatch(field, other)?,
            },
        }

        Ok(())
    }

    fn finish(self) -> ArrayRef {
        match self {
            Self::Binary(mut builder) => Arc::new(builder.finish()),
            Self::Bool(mut builder) => Arc::new(builder.finish()),
            Self::F32(mut builder) => Arc::new(builder.finish()),
            Self::F64(mut builder) => Arc::new(builder.finish()),
            Self::I32(mut builder) => Arc::new(builder.finish()),
            Self::I64(mut builder) => Arc::new(builder.finish()),
            Self::String(mut builder) => Arc::new(builder.finish()),
            Self::U32(mut builder) => Arc::new(builder.finish()),
            Self::U64(mut builder) => Arc::new(builder.finish()),
        }
    }
}

fn ensure_scalar_field(field: &FieldDescriptor) -> Result<()> {
    if field.is_list() || field.is_map() {
        bail!(
            "repeated or map field {} cannot be exposed as a scalar Arrow column",
            field.full_name()
        );
    }

    Ok(())
}

fn type_mismatch(field: &FieldDescriptor, value: &Value) -> Result<()> {
    bail!(
        "protobuf value for {} does not match descriptor kind {:?}: {value:?}",
        field.full_name(),
        field.kind()
    )
}

macro_rules! save_to_arrow {
    ($descriptor:expr, $messages:expr) => {{ $crate::arrow::save_to_arrow_batch($descriptor, $messages) }};
}

pub(crate) use save_to_arrow;
