//! Converts hitrace protobuf data into Arrow through runtime descriptors.

use std::{borrow::Cow, path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use arrow_array::{
    ArrayRef, RecordBatch,
    builder::{
        BooleanBuilder, Float32Builder, Float64Builder, Int32Builder, Int64Builder, StringBuilder,
        UInt32Builder, UInt64Builder,
    },
};
use arrow_schema::{DataType, Field, Schema};
use log::debug;
use prost_reflect::{
    DescriptorPool, DynamicMessage, FieldDescriptor, Kind, MessageDescriptor, Value,
};

use crate::mmap::with_mapped_file;

pub(crate) const HITRACE_TABLE: &str = "hitrace_event";

const HITRACE_TRACE_MESSAGE: &str = "kat.hitrace.HitraceTrace";
const HITRACE_EVENTS_FIELD: &str = "events";
const HITRACE_DESCRIPTOR_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/hitrace_descriptor.bin"));

pub(crate) fn load_hitrace_batch(path: &Path) -> Result<RecordBatch> {
    debug!("building hitrace datasource from {}", path.display());

    let descriptors = runtime_hitrace_descriptors()?;

    let trace = with_mapped_file(path, |bytes| {
        DynamicMessage::decode(descriptors.trace.clone(), bytes)
            .context("failed to decode hitrace protobuf")
    })?;
    let events = trace.get_field(&descriptors.events_field);
    let Value::List(event_values) = events.as_ref() else {
        bail!("hitrace events field is not a repeated field");
    };

    let mut builder =
        DynamicRecordBatchBuilder::new(descriptors.event.clone(), event_values.len())?;

    for value in event_values {
        let Value::Message(event) = value else {
            bail!("hitrace events field contains a non-message value");
        };
        builder.append(event)?;
    }

    let batch = builder.finish()?;

    debug!("built {} hitrace rows", batch.num_rows());
    Ok(batch)
}

fn runtime_hitrace_descriptors() -> Result<RuntimeHitraceDescriptors> {
    let trace_descriptor = runtime_hitrace_trace_descriptor()?;
    let events_field = runtime_hitrace_events_field(&trace_descriptor)?;
    let event_descriptor = message_field_descriptor(&events_field)?;

    Ok(RuntimeHitraceDescriptors {
        trace: trace_descriptor,
        events_field,
        event: event_descriptor,
    })
}

struct RuntimeHitraceDescriptors {
    trace: MessageDescriptor,
    events_field: FieldDescriptor,
    event: MessageDescriptor,
}

fn runtime_hitrace_trace_descriptor() -> Result<MessageDescriptor> {
    let pool = DescriptorPool::decode(HITRACE_DESCRIPTOR_BYTES)
        .context("failed to decode hitrace protobuf descriptor")?;

    pool.get_message_by_name(HITRACE_TRACE_MESSAGE)
        .with_context(|| format!("{HITRACE_TRACE_MESSAGE} descriptor is missing"))
}

fn runtime_hitrace_events_field(trace_descriptor: &MessageDescriptor) -> Result<FieldDescriptor> {
    let field = trace_descriptor
        .get_field_by_name(HITRACE_EVENTS_FIELD)
        .with_context(|| format!("{HITRACE_EVENTS_FIELD} field is missing"))?;

    if !field.is_list() {
        bail!("{HITRACE_EVENTS_FIELD} field must be repeated");
    }

    Ok(field)
}

fn message_field_descriptor(field: &FieldDescriptor) -> Result<MessageDescriptor> {
    let Kind::Message(descriptor) = field.kind() else {
        bail!("{} field must contain protobuf messages", field.full_name());
    };

    Ok(descriptor)
}

struct DynamicRecordBatchBuilder {
    schema: Arc<Schema>,
    fields: Vec<FieldDescriptor>,
    columns: Vec<DynamicColumnBuilder>,
}

impl DynamicRecordBatchBuilder {
    fn new(message_descriptor: MessageDescriptor, capacity: usize) -> Result<Self> {
        let fields = message_descriptor.fields().collect::<Vec<_>>();
        let schema = Arc::new(Schema::new(
            fields
                .iter()
                .map(DynamicColumnBuilder::field_schema)
                .collect::<Result<Vec<_>>>()?,
        ));
        let columns = fields
            .iter()
            .map(|field| DynamicColumnBuilder::new(field, capacity))
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
            .map(DynamicColumnBuilder::finish)
            .collect::<Vec<_>>();

        Ok(RecordBatch::try_new(self.schema, arrays)?)
    }
}

enum DynamicColumnBuilder {
    Bool(BooleanBuilder),
    F32(Float32Builder),
    F64(Float64Builder),
    I32(Int32Builder),
    I64(Int64Builder),
    String(StringBuilder),
    U32(UInt32Builder),
    U64(UInt64Builder),
}

impl DynamicColumnBuilder {
    fn new(field: &FieldDescriptor, capacity: usize) -> Result<Self> {
        ensure_scalar_field(field)?;

        Ok(match field.kind() {
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
            Self::Bool(builder) => {
                let Value::Bool(value) = value.as_ref() else {
                    type_mismatch(field, value.as_ref())?;
                    unreachable!();
                };
                builder.append_value(*value);
            }
            Self::F32(builder) => {
                let Value::F32(value) = value.as_ref() else {
                    type_mismatch(field, value.as_ref())?;
                    unreachable!();
                };
                builder.append_value(*value);
            }
            Self::F64(builder) => {
                let Value::F64(value) = value.as_ref() else {
                    type_mismatch(field, value.as_ref())?;
                    unreachable!();
                };
                builder.append_value(*value);
            }
            Self::I32(builder) => match value.as_ref() {
                Value::I32(value) | Value::EnumNumber(value) => builder.append_value(*value),
                other => {
                    type_mismatch(field, other)?;
                    unreachable!();
                }
            },
            Self::I64(builder) => {
                let Value::I64(value) = value.as_ref() else {
                    type_mismatch(field, value.as_ref())?;
                    unreachable!();
                };
                builder.append_value(*value);
            }
            Self::String(builder) => {
                let Value::String(value) = value.as_ref() else {
                    type_mismatch(field, value.as_ref())?;
                    unreachable!();
                };
                builder.append_value(value);
            }
            Self::U32(builder) => {
                let Value::U32(value) = value.as_ref() else {
                    type_mismatch(field, value.as_ref())?;
                    unreachable!();
                };
                builder.append_value(*value);
            }
            Self::U64(builder) => {
                let Value::U64(value) = value.as_ref() else {
                    type_mismatch(field, value.as_ref())?;
                    unreachable!();
                };
                builder.append_value(*value);
            }
        }

        Ok(())
    }

    fn finish(self) -> ArrayRef {
        match self {
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

#[cfg(test)]
mod tests {
    use super::runtime_hitrace_descriptors;

    #[test]
    fn runtime_descriptor_exposes_hitrace_event_fields() {
        let descriptors = runtime_hitrace_descriptors().expect("descriptor is available");
        let fields = descriptors
            .event
            .fields()
            .map(|field| field.name().to_owned())
            .collect::<Vec<_>>();

        assert_eq!(
            fields,
            ["timestamp_ns", "pid", "tid", "tag", "message", "cpu"]
        );
    }
}
