//! Converts Arrow query results into the JSON array returned by datasource APIs.

use anyhow::{Result, bail};
use arrow_array::{
    Array, BinaryArray, BooleanArray, Float32Array, Float64Array, Int32Array, Int64Array,
    RecordBatch, StringArray, UInt32Array, UInt64Array,
};
use arrow_schema::DataType;
use serde_json::{Map, Number, Value};

pub(crate) fn batches_to_json(batches: &[RecordBatch]) -> Result<Value> {
    let mut rows = Vec::new();

    for batch in batches {
        let schema = batch.schema();
        for row_index in 0..batch.num_rows() {
            let mut row = Map::new();
            for (column_index, field) in schema.fields().iter().enumerate() {
                row.insert(
                    field.name().clone(),
                    column_value(batch.column(column_index), field.data_type(), row_index)?,
                );
            }
            rows.push(Value::Object(row));
        }
    }

    Ok(Value::Array(rows))
}

fn column_value(array: &dyn Array, data_type: &DataType, row_index: usize) -> Result<Value> {
    if array.is_null(row_index) {
        return Ok(Value::Null);
    }

    match data_type {
        DataType::Binary => Ok(Value::String(bytes_to_hex(
            array
                .as_any()
                .downcast_ref::<BinaryArray>()
                .expect("binary array")
                .value(row_index),
        ))),
        DataType::Boolean => Ok(Value::Bool(
            array
                .as_any()
                .downcast_ref::<BooleanArray>()
                .expect("boolean array")
                .value(row_index),
        )),
        DataType::Int32 => Ok(Value::Number(Number::from(
            array
                .as_any()
                .downcast_ref::<Int32Array>()
                .expect("int32 array")
                .value(row_index),
        ))),
        DataType::Int64 => Ok(Value::Number(Number::from(
            array
                .as_any()
                .downcast_ref::<Int64Array>()
                .expect("int64 array")
                .value(row_index),
        ))),
        DataType::UInt32 => Ok(Value::Number(Number::from(
            array
                .as_any()
                .downcast_ref::<UInt32Array>()
                .expect("uint32 array")
                .value(row_index),
        ))),
        DataType::UInt64 => Ok(Value::Number(Number::from(
            array
                .as_any()
                .downcast_ref::<UInt64Array>()
                .expect("uint64 array")
                .value(row_index),
        ))),
        DataType::Float32 => float_to_json(
            array
                .as_any()
                .downcast_ref::<Float32Array>()
                .expect("float32 array")
                .value(row_index) as f64,
        ),
        DataType::Float64 => float_to_json(
            array
                .as_any()
                .downcast_ref::<Float64Array>()
                .expect("float64 array")
                .value(row_index),
        ),
        DataType::Utf8 => Ok(Value::String(
            array
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("utf8 array")
                .value(row_index)
                .to_owned(),
        )),
        other => bail!("unsupported json result type: {other:?}"),
    }
}

fn float_to_json(value: f64) -> Result<Value> {
    Ok(Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null))
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}
