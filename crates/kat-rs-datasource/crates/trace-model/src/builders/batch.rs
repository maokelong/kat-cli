use crate::trace_table_schema;
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::ArrowError;
use std::collections::BTreeMap;

pub struct TraceColumnArray {
    pub name: &'static str,
    pub array: ArrayRef,
}

impl TraceColumnArray {
    /// 创建用于契约驱动批数据组装的命名 Arrow 数组。
    pub fn new(name: &'static str, array: ArrayRef) -> Self {
        Self { name, array }
    }
}

/// 根据表契约对数组排序和校验，并构建 RecordBatch。
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

    let batch = RecordBatch::try_new(schema, ordered)?;
    if batch.num_rows() == 0 {
        return Err(ArrowError::SchemaError(format!(
            "trace table {table_name} has no rows"
        )));
    }

    Ok(batch)
}
