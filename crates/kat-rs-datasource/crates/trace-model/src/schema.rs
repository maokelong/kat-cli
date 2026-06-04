use crate::{schema_manifest, TraceDataType, TraceTableSchema};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use std::sync::Arc;

pub fn schema_for_table(table_name: &str) -> Option<SchemaRef> {
    schema_manifest()
        .table(table_name)
        .map(schema_from_manifest)
}

fn schema_from_manifest(table: &TraceTableSchema) -> SchemaRef {
    let fields = table
        .columns
        .iter()
        .map(|column| {
            Field::new(
                column.name.as_str(),
                arrow_data_type(column.data_type),
                column.nullable,
            )
        })
        .collect::<Vec<_>>();

    Arc::new(Schema::new(fields))
}

fn arrow_data_type(data_type: TraceDataType) -> DataType {
    match data_type {
        TraceDataType::Boolean => DataType::Boolean,
        TraceDataType::Float64 => DataType::Float64,
        TraceDataType::Int32 => DataType::Int32,
        TraceDataType::Int64 => DataType::Int64,
        TraceDataType::UInt32 => DataType::UInt32,
        TraceDataType::UInt64 => DataType::UInt64,
        TraceDataType::Utf8 => DataType::Utf8,
    }
}
