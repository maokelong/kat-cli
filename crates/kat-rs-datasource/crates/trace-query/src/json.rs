use crate::{QueryColumn, QueryResult, QueryStats, TraceEngineError, TraceResult, SCHEMA_VERSION};
use arrow_array::{
    Array, BooleanArray, Float64Array, Int32Array, Int64Array, LargeStringArray, RecordBatch,
    StringArray, StringViewArray, UInt32Array, UInt64Array,
};
use arrow_schema::DataType;
use serde_json::{Map, Value};

pub fn batches_to_query_result(
    batches: &[RecordBatch],
    max_inline_rows: usize,
) -> TraceResult<QueryResult> {
    let columns = batches
        .first()
        .map(|batch| {
            batch
                .schema()
                .fields()
                .iter()
                .map(|field| QueryColumn {
                    name: field.name().to_string(),
                    data_type: field.data_type().to_string(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut rows = Vec::new();
    let mut truncated = false;

    for batch in batches {
        for row_idx in 0..batch.num_rows() {
            if rows.len() >= max_inline_rows {
                truncated = true;
                break;
            }
            rows.push(batch_row_to_json(batch, row_idx)?);
        }
        if truncated {
            break;
        }
    }

    let status = if rows.is_empty() {
        "empty_result"
    } else {
        "ok"
    };

    Ok(QueryResult {
        status: status.to_string(),
        schema_version: SCHEMA_VERSION.to_string(),
        columns,
        stats: QueryStats {
            rows_returned: rows.len(),
            truncated,
        },
        rows,
    })
}

fn batch_row_to_json(batch: &RecordBatch, row_idx: usize) -> TraceResult<Value> {
    let mut object = Map::new();
    let schema = batch.schema();

    for (col_idx, field) in schema.fields().iter().enumerate() {
        let value =
            array_value_to_json(batch.column(col_idx).as_ref(), field.data_type(), row_idx)?;
        object.insert(field.name().to_string(), value);
    }

    Ok(Value::Object(object))
}

fn array_value_to_json(
    array: &dyn Array,
    data_type: &DataType,
    row_idx: usize,
) -> TraceResult<Value> {
    if array.is_null(row_idx) {
        return Ok(Value::Null);
    }

    match data_type {
        DataType::Boolean => {
            typed_value::<BooleanArray, _>(array, |a| Value::Bool(a.value(row_idx)))
        }
        DataType::Int32 => typed_value::<Int32Array, _>(array, |a| a.value(row_idx).into()),
        DataType::Int64 => typed_value::<Int64Array, _>(array, |a| a.value(row_idx).into()),
        DataType::UInt32 => typed_value::<UInt32Array, _>(array, |a| a.value(row_idx).into()),
        DataType::UInt64 => typed_value::<UInt64Array, _>(array, |a| a.value(row_idx).into()),
        DataType::Float64 => typed_value::<Float64Array, _>(array, |a| {
            serde_json::Number::from_f64(a.value(row_idx))
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }),
        DataType::Utf8 => {
            typed_value::<StringArray, _>(array, |a| Value::String(a.value(row_idx).to_string()))
        }
        DataType::LargeUtf8 => typed_value::<LargeStringArray, _>(array, |a| {
            Value::String(a.value(row_idx).to_string())
        }),
        DataType::Utf8View => typed_value::<StringViewArray, _>(array, |a| {
            Value::String(a.value(row_idx).to_string())
        }),
        other => Err(TraceEngineError::UnsupportedSchema(format!(
            "JSON conversion does not support Arrow type {other}"
        ))),
    }
}

fn typed_value<A, F>(array: &dyn Array, f: F) -> TraceResult<Value>
where
    A: Array + 'static,
    F: FnOnce(&A) -> Value,
{
    let typed = array.as_any().downcast_ref::<A>().ok_or_else(|| {
        TraceEngineError::Engine("Arrow array type did not match schema".to_string())
    })?;
    Ok(f(typed))
}
