use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result, bail};
use arrow_array::{
    ArrayRef, RecordBatch,
    builder::{
        ArrayBuilder, BinaryBuilder, BooleanBuilder, Float32Builder, Float64Builder, Int32Builder,
        Int64Builder, StringBuilder, StructBuilder, UInt32Builder, UInt64Builder, make_builder,
    },
};
use arrow_schema::{DataType, Field, Schema};

use crate::{
    dataset_writer::{DatasetTableWriter, DatasetWriter},
    payload_value::PayloadValue,
};

use super::{
    plan::ExpansionPlanItem,
    row::{ColumnProjection, ColumnSpec, ColumnType, OneofVariantName},
};

const RELATIONAL_TABLE_BUFFER_MAX_ROWS: usize = 64 * 1024;
const RELATIONAL_TABLE_BUFFER_MAX_ESTIMATED_BYTES: usize = 32 * 1024 * 1024;
const RELATIONAL_PARQUET_WRITE_QUEUE_CAPACITY: usize = 0;
const RELATIONAL_ROW_FIXED_BYTES: usize = 128;
const CELL_VALUE_FIXED_BYTES: usize = 32;

pub(super) struct TableBuffer {
    columns: Vec<ColumnSpec>,
    parent_table: Option<String>,
    builders: TableColumnBuilders,
    next_row_index: u64,
    buffered_rows: usize,
    estimated_bytes: usize,
}

struct TableChunk {
    table_name: String,
    rows: usize,
    batch: RecordBatch,
}

pub(super) struct TableColumnBuilders {
    source_index: UInt64Builder,
    parent_index: UInt64Builder,
    row_index: UInt64Builder,
    values: Vec<ColumnBuilder>,
}

enum ColumnBuilder {
    Binary(BinaryBuilder),
    Bool(BooleanBuilder),
    I32(Int32Builder),
    I64(Int64Builder),
    U32(UInt32Builder),
    U64(UInt64Builder),
    F32(Float32Builder),
    F64(Float64Builder),
    String(StringBuilder),
    Nested {
        column_type: ColumnType,
        builder: Box<dyn ArrayBuilder>,
    },
}

pub(super) fn push_row_to_table(
    writer: &ParquetWriteWorker,
    tables: &mut BTreeMap<String, TableBuffer>,
    item: &ExpansionPlanItem,
    columns: &[ColumnSpec],
    source_index: u64,
    parent_index: Option<u64>,
    append_values: impl FnOnce(&mut TableColumnBuilders) -> Result<(usize, usize)>,
) -> Result<u64> {
    let table = tables
        .entry(item.output_table.clone())
        .or_insert_with(|| TableBuffer::new(columns, item.parent_table.clone()));
    if table.columns != columns {
        bail!(
            "relational table {} received incompatible columns",
            item.output_table
        );
    }
    if table.parent_table != item.parent_table {
        bail!(
            "relational table {} received incompatible parent table",
            item.output_table
        );
    }

    let row_index = table.next_row_index;
    table.next_row_index += 1;
    let row_bytes = table.append_row_with(source_index, parent_index, row_index, append_values)?;
    table.estimated_bytes = table.estimated_bytes.saturating_add(row_bytes);
    let should_flush = table.should_flush();
    let table_name = item.output_table.clone();
    if should_flush {
        flush_table(writer, tables, &table_name)?;
    }

    Ok(row_index)
}

pub(super) struct ParquetWriteWorker {
    sender: Option<SyncSender<TableChunk>>,
    handle: Option<JoinHandle<Result<DatasetWriter>>>,
}

pub(super) fn flush_table(
    writer: &ParquetWriteWorker,
    tables: &mut BTreeMap<String, TableBuffer>,
    table_name: &str,
) -> Result<()> {
    let Some(chunk) = finish_table_chunk(tables, table_name)? else {
        return Ok(());
    };
    writer.write(chunk)
}

fn finish_table_chunk(
    tables: &mut BTreeMap<String, TableBuffer>,
    table_name: &str,
) -> Result<Option<TableChunk>> {
    let Some(table) = tables.get_mut(table_name) else {
        return Ok(None);
    };
    if table.buffered_rows == 0 {
        return Ok(None);
    };

    let rows = table.buffered_rows;
    let batch = table
        .finish_record_batch()
        .with_context(|| format!("failed to build relational table {table_name}"))?;
    table.buffered_rows = 0;
    table.estimated_bytes = 0;

    Ok(Some(TableChunk {
        table_name: table_name.to_string(),
        rows,
        batch,
    }))
}

impl ParquetWriteWorker {
    pub(super) fn new(dataset_writer: DatasetWriter) -> Result<Self> {
        let (sender, receiver) = sync_channel(RELATIONAL_PARQUET_WRITE_QUEUE_CAPACITY);
        let handle = thread::Builder::new()
            .name("kat-parquet-writer".to_string())
            .spawn(move || write_table_chunks(dataset_writer, receiver))
            .context("failed to start relational Parquet writer")?;
        Ok(Self {
            sender: Some(sender),
            handle: Some(handle),
        })
    }

    fn write(&self, chunk: TableChunk) -> Result<()> {
        let sender = self
            .sender
            .as_ref()
            .context("relational Parquet writer is closed")?;
        sender
            .send(chunk)
            .map_err(|_| anyhow::anyhow!("relational Parquet writer stopped unexpectedly"))
    }

    pub(super) fn finish(mut self) -> Result<DatasetWriter> {
        self.sender.take();
        join_writer(self.handle.take())
    }
}

impl Drop for ParquetWriteWorker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn write_table_chunks(
    mut dataset_writer: DatasetWriter,
    receiver: Receiver<TableChunk>,
) -> Result<DatasetWriter> {
    let mut writers = BTreeMap::<String, DatasetTableWriter>::new();
    for chunk in receiver {
        if chunk.rows == 0 {
            continue;
        }
        if !writers.contains_key(&chunk.table_name) {
            let writer = dataset_writer.begin_table(&chunk.table_name, chunk.batch.schema())?;
            writers.insert(chunk.table_name.clone(), writer);
        }
        writers
            .get_mut(&chunk.table_name)
            .with_context(|| format!("missing Parquet writer for {}", chunk.table_name))?
            .write(&chunk.batch)?;
    }

    for (_, writer) in writers {
        writer.finish()?;
    }
    Ok(dataset_writer)
}

fn join_writer(handle: Option<JoinHandle<Result<DatasetWriter>>>) -> Result<DatasetWriter> {
    handle
        .context("relational Parquet writer handle is missing")?
        .join()
        .map_err(|_| anyhow::anyhow!("relational Parquet writer panicked"))?
}

impl TableBuffer {
    fn new(columns: &[ColumnSpec], parent_table: Option<String>) -> Self {
        let builders = TableColumnBuilders::new(columns);
        Self {
            columns: columns.to_vec(),
            parent_table,
            builders,
            next_row_index: 0,
            buffered_rows: 0,
            estimated_bytes: 0,
        }
    }

    fn append_row_with(
        &mut self,
        source_index: u64,
        parent_index: Option<u64>,
        row_index: u64,
        append_values: impl FnOnce(&mut TableColumnBuilders) -> Result<(usize, usize)>,
    ) -> Result<usize> {
        self.builders
            .append_common(source_index, parent_index, row_index);
        let (value_count, value_bytes) = append_values(&mut self.builders)?;
        if value_count != self.columns.len() {
            bail!(
                "relational row has {value_count} values but schema has {} columns",
                self.columns.len()
            );
        }
        self.buffered_rows += 1;
        Ok(RELATIONAL_ROW_FIXED_BYTES + value_bytes)
    }

    fn finish_record_batch(&mut self) -> Result<RecordBatch> {
        self.builders.finish_record_batch(&self.columns)
    }

    fn should_flush(&self) -> bool {
        self.buffered_rows >= RELATIONAL_TABLE_BUFFER_MAX_ROWS
            || self.estimated_bytes >= RELATIONAL_TABLE_BUFFER_MAX_ESTIMATED_BYTES
    }
}

impl TableColumnBuilders {
    fn new(columns: &[ColumnSpec]) -> Self {
        Self {
            source_index: UInt64Builder::new(),
            parent_index: UInt64Builder::new(),
            row_index: UInt64Builder::new(),
            values: columns.iter().map(ColumnBuilder::new).collect(),
        }
    }

    fn append_common(&mut self, source_index: u64, parent_index: Option<u64>, row_index: u64) {
        self.source_index.append_value(source_index);
        append_u64_option(&mut self.parent_index, parent_index);
        self.row_index.append_value(row_index);
    }

    pub(super) fn append_payload_value(
        &mut self,
        column_index: usize,
        column: &ColumnSpec,
        value: &PayloadValue,
    ) -> Result<usize> {
        let Some(builder) = self.values.get_mut(column_index) else {
            bail!("missing value builder for column {}", column.name);
        };
        builder.append_payload_cell(value, column)
    }
    fn finish_record_batch(&mut self, columns: &[ColumnSpec]) -> Result<RecordBatch> {
        let mut fields = vec![
            Field::new("source_index", DataType::UInt64, false),
            Field::new("parent_index", DataType::UInt64, true),
            Field::new("row_index", DataType::UInt64, false),
        ];
        fields.extend(
            columns
                .iter()
                .map(|column| Field::new(&column.name, arrow_type(&column.column_type), true)),
        );

        let mut arrays: Vec<ArrayRef> = vec![
            Arc::new(self.source_index.finish()),
            Arc::new(self.parent_index.finish()),
            Arc::new(self.row_index.finish()),
        ];
        for builder in &mut self.values {
            arrays.push(builder.finish());
        }

        Ok(RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)?)
    }
}

impl ColumnBuilder {
    fn new(column: &ColumnSpec) -> Self {
        match &column.column_type {
            ColumnType::Binary => Self::Binary(BinaryBuilder::new()),
            ColumnType::Bool => Self::Bool(BooleanBuilder::new()),
            ColumnType::I32 => Self::I32(Int32Builder::new()),
            ColumnType::I64 => Self::I64(Int64Builder::new()),
            ColumnType::U32 => Self::U32(UInt32Builder::new()),
            ColumnType::U64 => Self::U64(UInt64Builder::new()),
            ColumnType::F32 => Self::F32(Float32Builder::new()),
            ColumnType::F64 => Self::F64(Float64Builder::new()),
            ColumnType::String => Self::String(StringBuilder::new()),
            ColumnType::Struct(_) => Self::Nested {
                column_type: column.column_type.clone(),
                builder: make_builder(&arrow_type(&column.column_type), 0),
            },
        }
    }

    fn append_payload_cell(&mut self, value: &PayloadValue, column: &ColumnSpec) -> Result<usize> {
        let column_name = column.name.as_str();
        match &column.projection {
            ColumnProjection::EnumName(enum_values) => {
                return append_enum_name(self, value, enum_values, column_name);
            }
            ColumnProjection::OneofName(variants) => {
                return append_oneof_name(self, value, variants, column_name);
            }
            ColumnProjection::Direct => {}
        }

        if value.is_null() {
            match self {
                Self::Binary(builder) => builder.append_null(),
                Self::Bool(builder) => builder.append_null(),
                Self::I32(builder) => builder.append_null(),
                Self::I64(builder) => builder.append_null(),
                Self::U32(builder) => builder.append_null(),
                Self::U64(builder) => builder.append_null(),
                Self::F32(builder) => builder.append_null(),
                Self::F64(builder) => builder.append_null(),
                Self::String(builder) => builder.append_null(),
                Self::Nested {
                    column_type: _,
                    builder,
                } => {
                    return append_nested_value(builder.as_mut(), column, value, column_name);
                }
            }
            return Ok(CELL_VALUE_FIXED_BYTES);
        }

        match (self, &column.column_type) {
            (Self::Binary(builder), ColumnType::Binary) => append_payload_bytes(builder, value),
            (Self::Bool(builder), ColumnType::Bool) => value
                .as_bool()
                .map(|value| {
                    builder.append_value(value);
                    CELL_VALUE_FIXED_BYTES + 1
                })
                .context("expected bool value"),
            (Self::I32(builder), ColumnType::I32) => value
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .map(|value| {
                    builder.append_value(value);
                    CELL_VALUE_FIXED_BYTES + 4
                })
                .context("expected i32 value"),
            (Self::I64(builder), ColumnType::I64) => value
                .as_i64()
                .map(|value| {
                    builder.append_value(value);
                    CELL_VALUE_FIXED_BYTES + 8
                })
                .context("expected i64 value"),
            (Self::U32(builder), ColumnType::U32) => value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .map(|value| {
                    builder.append_value(value);
                    CELL_VALUE_FIXED_BYTES + 4
                })
                .context("expected u32 value"),
            (Self::U64(builder), ColumnType::U64) => value
                .as_u64()
                .map(|value| {
                    builder.append_value(value);
                    CELL_VALUE_FIXED_BYTES + 8
                })
                .context("expected u64 value"),
            (Self::F32(builder), ColumnType::F32) => value
                .as_f64()
                .map(|value| {
                    builder.append_value(value as f32);
                    CELL_VALUE_FIXED_BYTES + 4
                })
                .context("expected f32 value"),
            (Self::F64(builder), ColumnType::F64) => value
                .as_f64()
                .map(|value| {
                    builder.append_value(value);
                    CELL_VALUE_FIXED_BYTES + 8
                })
                .context("expected f64 value"),
            (Self::String(builder), ColumnType::String) => value
                .as_str()
                .map(|value| {
                    builder.append_value(value);
                    CELL_VALUE_FIXED_BYTES + value.len()
                })
                .context("expected string value"),
            (
                Self::Nested {
                    column_type: actual_type,
                    builder,
                },
                expected_type,
            ) if &*actual_type == expected_type => {
                append_nested_value(builder.as_mut(), column, value, column_name)
            }
            _ => bail!("column {column_name} received incompatible value"),
        }
    }

    fn finish(&mut self) -> ArrayRef {
        match self {
            Self::Binary(builder) => Arc::new(builder.finish()),
            Self::Bool(builder) => Arc::new(builder.finish()),
            Self::I32(builder) => Arc::new(builder.finish()),
            Self::I64(builder) => Arc::new(builder.finish()),
            Self::U32(builder) => Arc::new(builder.finish()),
            Self::U64(builder) => Arc::new(builder.finish()),
            Self::F32(builder) => Arc::new(builder.finish()),
            Self::F64(builder) => Arc::new(builder.finish()),
            Self::String(builder) => Arc::new(builder.finish()),
            Self::Nested { builder, .. } => builder.finish(),
        }
    }
}

fn append_payload_bytes(builder: &mut BinaryBuilder, value: &PayloadValue) -> Result<usize> {
    if let Some(bytes) = value.as_binary() {
        builder.append_value(bytes);
        return Ok(CELL_VALUE_FIXED_BYTES + bytes.len());
    }

    let value = value.as_str().context("expected binary value")?;
    builder.append_value(value.as_bytes());
    Ok(CELL_VALUE_FIXED_BYTES + value.len())
}

fn append_nested_value(
    builder: &mut dyn ArrayBuilder,
    column: &ColumnSpec,
    value: &PayloadValue,
    column_name: &str,
) -> Result<usize> {
    match &column.projection {
        ColumnProjection::EnumName(enum_values) => {
            return append_nested_enum_name(builder, value, enum_values, column_name);
        }
        ColumnProjection::OneofName(variants) => {
            return append_nested_oneof_name(builder, value, variants, column_name);
        }
        ColumnProjection::Direct => {}
    }

    if value.is_null() {
        append_nested_null(builder, &column.column_type, column_name)?;
        return Ok(CELL_VALUE_FIXED_BYTES);
    }

    match &column.column_type {
        ColumnType::Binary => {
            append_payload_bytes(builder_mut::<BinaryBuilder>(builder, column_name)?, value)
        }
        ColumnType::Bool => value
            .as_bool()
            .map(|value| {
                builder_mut::<BooleanBuilder>(builder, column_name)
                    .expect("builder type was created from column type")
                    .append_value(value);
                CELL_VALUE_FIXED_BYTES + 1
            })
            .context("expected bool value"),
        ColumnType::I32 => value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .map(|value| {
                builder_mut::<Int32Builder>(builder, column_name)
                    .expect("builder type was created from column type")
                    .append_value(value);
                CELL_VALUE_FIXED_BYTES + 4
            })
            .context("expected i32 value"),
        ColumnType::I64 => value
            .as_i64()
            .map(|value| {
                builder_mut::<Int64Builder>(builder, column_name)
                    .expect("builder type was created from column type")
                    .append_value(value);
                CELL_VALUE_FIXED_BYTES + 8
            })
            .context("expected i64 value"),
        ColumnType::U32 => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(|value| {
                builder_mut::<UInt32Builder>(builder, column_name)
                    .expect("builder type was created from column type")
                    .append_value(value);
                CELL_VALUE_FIXED_BYTES + 4
            })
            .context("expected u32 value"),
        ColumnType::U64 => value
            .as_u64()
            .map(|value| {
                builder_mut::<UInt64Builder>(builder, column_name)
                    .expect("builder type was created from column type")
                    .append_value(value);
                CELL_VALUE_FIXED_BYTES + 8
            })
            .context("expected u64 value"),
        ColumnType::F32 => value
            .as_f64()
            .map(|value| {
                builder_mut::<Float32Builder>(builder, column_name)
                    .expect("builder type was created from column type")
                    .append_value(value as f32);
                CELL_VALUE_FIXED_BYTES + 4
            })
            .context("expected f32 value"),
        ColumnType::F64 => value
            .as_f64()
            .map(|value| {
                builder_mut::<Float64Builder>(builder, column_name)
                    .expect("builder type was created from column type")
                    .append_value(value);
                CELL_VALUE_FIXED_BYTES + 8
            })
            .context("expected f64 value"),
        ColumnType::String => {
            let value = value.as_str().context("expected string value")?;
            builder_mut::<StringBuilder>(builder, column_name)?.append_value(value);
            Ok(CELL_VALUE_FIXED_BYTES + value.len())
        }
        ColumnType::Struct(columns) => {
            let struct_builder = builder_mut::<StructBuilder>(builder, column_name)?;
            if !value.is_object() {
                bail!("column {column_name} expected message value");
            }
            let mut estimated_bytes = CELL_VALUE_FIXED_BYTES;
            for (index, column) in columns.iter().enumerate() {
                let field_value =
                    payload_child(value, &column.source_name).unwrap_or(&PayloadValue::Null);
                estimated_bytes += append_nested_value(
                    struct_builder.field_builders_mut()[index].as_mut(),
                    column,
                    field_value,
                    &column.name,
                )?;
            }
            struct_builder.append(true);
            Ok(estimated_bytes)
        }
    }
}

fn append_nested_null(
    builder: &mut dyn ArrayBuilder,
    column_type: &ColumnType,
    column_name: &str,
) -> Result<()> {
    match column_type {
        ColumnType::Binary => builder_mut::<BinaryBuilder>(builder, column_name)?.append_null(),
        ColumnType::Bool => builder_mut::<BooleanBuilder>(builder, column_name)?.append_null(),
        ColumnType::I32 => builder_mut::<Int32Builder>(builder, column_name)?.append_null(),
        ColumnType::I64 => builder_mut::<Int64Builder>(builder, column_name)?.append_null(),
        ColumnType::U32 => builder_mut::<UInt32Builder>(builder, column_name)?.append_null(),
        ColumnType::U64 => builder_mut::<UInt64Builder>(builder, column_name)?.append_null(),
        ColumnType::F32 => builder_mut::<Float32Builder>(builder, column_name)?.append_null(),
        ColumnType::F64 => builder_mut::<Float64Builder>(builder, column_name)?.append_null(),
        ColumnType::String => builder_mut::<StringBuilder>(builder, column_name)?.append_null(),
        ColumnType::Struct(columns) => {
            let struct_builder = builder_mut::<StructBuilder>(builder, column_name)?;
            for (index, column) in columns.iter().enumerate() {
                append_nested_null(
                    struct_builder.field_builders_mut()[index].as_mut(),
                    &column.column_type,
                    &column.name,
                )?;
            }
            struct_builder.append(false);
        }
    }
    Ok(())
}

fn append_enum_name(
    builder: &mut ColumnBuilder,
    value: &PayloadValue,
    enum_values: &[super::descriptor::EnumValueDescriptor],
    column_name: &str,
) -> Result<usize> {
    let ColumnBuilder::String(builder) = builder else {
        bail!("column {column_name} received incompatible builder");
    };
    append_enum_name_to_builder(builder, value, enum_values)
}

fn append_nested_enum_name(
    builder: &mut dyn ArrayBuilder,
    value: &PayloadValue,
    enum_values: &[super::descriptor::EnumValueDescriptor],
    column_name: &str,
) -> Result<usize> {
    let builder = builder_mut::<StringBuilder>(builder, column_name)?;
    append_enum_name_to_builder(builder, value, enum_values)
}

fn append_enum_name_to_builder(
    builder: &mut StringBuilder,
    value: &PayloadValue,
    enum_values: &[super::descriptor::EnumValueDescriptor],
) -> Result<usize> {
    if value.is_null() {
        builder.append_null();
        return Ok(CELL_VALUE_FIXED_BYTES);
    }
    let number = value
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .context("expected enum value")?;
    let name = enum_values
        .iter()
        .find(|enum_value| enum_value.number == number)
        .map(|enum_value| enum_value.name);
    match name {
        Some(name) => builder.append_value(name),
        None => builder.append_null(),
    }
    Ok(CELL_VALUE_FIXED_BYTES + name.map(str::len).unwrap_or(0))
}

fn append_oneof_name(
    builder: &mut ColumnBuilder,
    value: &PayloadValue,
    variants: &[OneofVariantName],
    column_name: &str,
) -> Result<usize> {
    let ColumnBuilder::String(builder) = builder else {
        bail!("column {column_name} received incompatible builder");
    };
    append_oneof_name_to_builder(builder, value, variants)
}

fn append_nested_oneof_name(
    builder: &mut dyn ArrayBuilder,
    value: &PayloadValue,
    variants: &[OneofVariantName],
    column_name: &str,
) -> Result<usize> {
    let builder = builder_mut::<StringBuilder>(builder, column_name)?;
    append_oneof_name_to_builder(builder, value, variants)
}

fn append_oneof_name_to_builder(
    builder: &mut StringBuilder,
    value: &PayloadValue,
    variants: &[OneofVariantName],
) -> Result<usize> {
    let selected = selected_oneof_name(value, variants);
    match selected {
        Some(name) => {
            builder.append_value(name);
            Ok(CELL_VALUE_FIXED_BYTES + name.len())
        }
        None => {
            builder.append_null();
            Ok(CELL_VALUE_FIXED_BYTES)
        }
    }
}

fn selected_oneof_name<'a>(
    value: &PayloadValue,
    variants: &'a [OneofVariantName],
) -> Option<&'a str> {
    let selected = value.as_object()?.first()?.name();
    variants
        .iter()
        .find(|variant| {
            variant.field_name == selected || variant.serialized_name.as_str() == selected
        })
        .map(|variant| variant.field_name)
}

fn payload_child<'a>(value: &'a PayloadValue, field_name: &str) -> Option<&'a PayloadValue> {
    value
        .as_object()?
        .iter()
        .find(|field| field.name() == field_name)
        .map(|field| field.value())
}

fn builder_mut<'a, T: ArrayBuilder>(
    builder: &'a mut dyn ArrayBuilder,
    column_name: &str,
) -> Result<&'a mut T> {
    builder
        .as_any_mut()
        .downcast_mut::<T>()
        .with_context(|| format!("column {column_name} received incompatible builder"))
}

fn append_u64_option(builder: &mut UInt64Builder, value: Option<u64>) {
    match value {
        Some(value) => builder.append_value(value),
        None => builder.append_null(),
    }
}

fn arrow_type(column_type: &ColumnType) -> DataType {
    match column_type {
        ColumnType::Binary => DataType::Binary,
        ColumnType::Bool => DataType::Boolean,
        ColumnType::I32 => DataType::Int32,
        ColumnType::I64 => DataType::Int64,
        ColumnType::U32 => DataType::UInt32,
        ColumnType::U64 => DataType::UInt64,
        ColumnType::F32 => DataType::Float32,
        ColumnType::F64 => DataType::Float64,
        ColumnType::String => DataType::Utf8,
        ColumnType::Struct(columns) => DataType::Struct(
            columns
                .iter()
                .map(|column| {
                    Arc::new(Field::new(
                        &column.name,
                        arrow_type(&column.column_type),
                        true,
                    ))
                })
                .collect::<Vec<_>>()
                .into(),
        ),
    }
}
