use crate::trace_table_schema;
use arrow_array::{
    ArrayRef, BooleanArray, Float64Array, Int32Array, Int64Array, RecordBatch, StringArray,
    UInt32Array, UInt64Array,
};
use arrow_schema::{ArrowError, DataType};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct TraceColumnArray {
    pub name: &'static str,
    pub array: ArrayRef,
}

impl TraceColumnArray {
    /// Creates a named Arrow array for contract-driven batch assembly.
    pub fn new(name: &'static str, array: ArrayRef) -> Self {
        Self { name, array }
    }
}

/// Builds a RecordBatch by ordering and validating arrays against a table contract.
pub fn assemble_trace_table_batch(
    table_name: &str,
    columns: Vec<TraceColumnArray>,
) -> Result<RecordBatch, ArrowError> {
    let schema = trace_table_schema(table_name)
        .ok_or_else(|| ArrowError::SchemaError(format!("unknown trace table: {table_name}")))?;
    let mut by_name = BTreeMap::new();

    for column in columns {
        if by_name.insert(column.name, column.array).is_some() {
            return Err(ArrowError::SchemaError(format!(
                "duplicate column {table_name}.{}",
                column.name
            )));
        }
    }

    let mut ordered = Vec::with_capacity(schema.fields().len());
    for field in schema.fields() {
        let array = by_name.remove(field.name().as_str()).ok_or_else(|| {
            ArrowError::SchemaError(format!("missing column {table_name}.{}", field.name()))
        })?;
        if array.data_type() != field.data_type() {
            return Err(ArrowError::SchemaError(format!(
                "column type mismatch {table_name}.{} expected {} got {}",
                field.name(),
                field.data_type(),
                array.data_type()
            )));
        }
        ordered.push(array);
    }

    if let Some(extra) = by_name.keys().next() {
        return Err(ArrowError::SchemaError(format!(
            "extra column {table_name}.{extra}"
        )));
    }

    RecordBatch::try_new(schema, ordered)
}

/// Builds an empty RecordBatch for a registered table contract.
pub fn empty_trace_table_batch(table_name: &str) -> Result<RecordBatch, ArrowError> {
    let schema = trace_table_schema(table_name)
        .ok_or_else(|| ArrowError::SchemaError(format!("unknown trace table: {table_name}")))?;
    let columns = schema
        .fields()
        .iter()
        .map(|field| empty_array(field.data_type()))
        .collect::<Result<Vec<_>, _>>()?;

    RecordBatch::try_new(schema, columns)
}

/// Creates an empty Arrow array for a supported contract column type.
fn empty_array(data_type: &DataType) -> Result<ArrayRef, ArrowError> {
    let array = match data_type {
        DataType::Boolean => Arc::new(BooleanArray::from(Vec::<bool>::new())) as ArrayRef,
        DataType::Float64 => Arc::new(Float64Array::from(Vec::<f64>::new())) as ArrayRef,
        DataType::Int32 => Arc::new(Int32Array::from(Vec::<i32>::new())) as ArrayRef,
        DataType::Int64 => Arc::new(Int64Array::from(Vec::<i64>::new())) as ArrayRef,
        DataType::UInt32 => Arc::new(UInt32Array::from(Vec::<u32>::new())) as ArrayRef,
        DataType::UInt64 => Arc::new(UInt64Array::from(Vec::<u64>::new())) as ArrayRef,
        DataType::Utf8 => Arc::new(StringArray::from(Vec::<&str>::new())) as ArrayRef,
        other => {
            return Err(ArrowError::SchemaError(format!(
                "unsupported empty trace column type: {other}"
            )));
        }
    };

    Ok(array)
}
