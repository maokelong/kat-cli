//! 构建期生成代码与运行时代码共享的表/字段契约。

use std::sync::Arc;

use arrow_schema::{DataType, Field, Schema, SchemaRef};

/// Arrow 构建运行时支持的列类型。
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ArrowType {
    Boolean,
    Int32,
    Int64,
    UInt32,
    UInt64,
    Float32,
    Float64,
    Utf8,
    Binary,
}

/// 单个 protobuf 字段到 Arrow 列的结构描述。
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct FieldSpec {
    pub name: &'static str,
    pub source: &'static str,
    pub arrow_type: ArrowType,
    pub nullable: bool,
    pub repeated: bool,
}

/// 单张 SQL 表的结构描述。
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct TableSpec {
    pub name: &'static str,
    pub source: &'static str,
    pub repeated_field: &'static str,
    pub fields: &'static [FieldSpec],
}

/// 根据表结构描述构建 Arrow schema。
pub fn schema_for_table(table: &TableSpec) -> SchemaRef {
    let fields = table
        .fields
        .iter()
        .map(|field| {
            Field::new(
                field.name,
                arrow_data_type(field.arrow_type),
                field.nullable,
            )
        })
        .collect::<Vec<_>>();

    Arc::new(Schema::new(fields))
}

/// 将通用 ArrowType 映射为 Arrow DataType。
fn arrow_data_type(arrow_type: ArrowType) -> DataType {
    match arrow_type {
        ArrowType::Boolean => DataType::Boolean,
        ArrowType::Int32 => DataType::Int32,
        ArrowType::Int64 => DataType::Int64,
        ArrowType::UInt32 => DataType::UInt32,
        ArrowType::UInt64 => DataType::UInt64,
        ArrowType::Float32 => DataType::Float32,
        ArrowType::Float64 => DataType::Float64,
        ArrowType::Utf8 => DataType::Utf8,
        ArrowType::Binary => DataType::Binary,
    }
}
