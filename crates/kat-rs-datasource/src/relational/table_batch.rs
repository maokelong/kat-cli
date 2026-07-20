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
        BinaryBuilder, BooleanBuilder, Float32Builder, Float64Builder, Int32Builder, Int64Builder,
        StringBuilder, UInt32Builder, UInt64Builder,
    },
};
use arrow_schema::{DataType, Field, Schema};

use crate::{
    dataset::{DatasetTableWriter, DatasetWriter},
    payload_value::PayloadValue,
};

use super::{
    plan::ExpansionPlanItem,
    row::{CellValue, ColumnSpec, ColumnType},
};

const RELATIONAL_TABLE_BUFFER_MAX_ROWS: usize = 64 * 1024;
const RELATIONAL_TABLE_BUFFER_MAX_ESTIMATED_BYTES: usize = 32 * 1024 * 1024;
const RELATIONAL_PARQUET_WRITE_QUEUE_CAPACITY: usize = 0;
const RELATIONAL_ROW_FIXED_BYTES: usize = 128;
const CELL_VALUE_FIXED_BYTES: usize = 32;

pub(super) struct TableBuffer {
    columns: Vec<ColumnSpec>,
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
        .or_insert_with(|| TableBuffer::new(columns));
    if table.columns != columns {
        bail!(
            "relational table {} received incompatible columns",
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
            let parquet_file_name = format!("{}.parquet", chunk.table_name);
            let writer = dataset_writer.start_table(
                &chunk.table_name,
                &parquet_file_name,
                chunk.batch.schema(),
            )?;
            writers.insert(chunk.table_name.clone(), writer);
        }
        writers
            .get_mut(&chunk.table_name)
            .with_context(|| format!("missing Parquet writer for {}", chunk.table_name))?
            .write(&chunk.batch)?;
    }

    for (_, writer) in writers {
        dataset_writer.add_table(writer.finish()?);
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
    fn new(columns: &[ColumnSpec]) -> Self {
        let builders = TableColumnBuilders::new(columns);
        Self {
            columns: columns.to_vec(),
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
            values: columns
                .iter()
                .map(|column| ColumnBuilder::new(column.column_type))
                .collect(),
        }
    }

    fn append_common(&mut self, source_index: u64, parent_index: Option<u64>, row_index: u64) {
        self.source_index.append_value(source_index);
        append_u64_option(&mut self.parent_index, parent_index);
        self.row_index.append_value(row_index);
    }

    pub(super) fn append_cell(
        &mut self,
        column_index: usize,
        column_name: &str,
        value: CellValue,
    ) -> Result<usize> {
        let Some(builder) = self.values.get_mut(column_index) else {
            bail!("missing value builder for column {column_name}");
        };
        builder.append_cell(value, column_name)
    }

    pub(super) fn append_payload_value(
        &mut self,
        column_index: usize,
        column_name: &str,
        value: &PayloadValue,
        column_type: ColumnType,
    ) -> Result<usize> {
        let Some(builder) = self.values.get_mut(column_index) else {
            bail!("missing value builder for column {column_name}");
        };
        builder.append_payload_cell(value, column_type, column_name)
    }

    pub(super) fn append_string_value(
        &mut self,
        column_index: usize,
        column_name: &str,
        value: Option<&str>,
    ) -> Result<usize> {
        let Some(builder) = self.values.get_mut(column_index) else {
            bail!("missing value builder for column {column_name}");
        };
        builder.append_string_value(value, column_name)
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
                .map(|column| Field::new(&column.name, arrow_type(column.column_type), true)),
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
    fn new(column_type: ColumnType) -> Self {
        match column_type {
            ColumnType::Binary => Self::Binary(BinaryBuilder::new()),
            ColumnType::Bool => Self::Bool(BooleanBuilder::new()),
            ColumnType::I32 => Self::I32(Int32Builder::new()),
            ColumnType::I64 => Self::I64(Int64Builder::new()),
            ColumnType::U32 => Self::U32(UInt32Builder::new()),
            ColumnType::U64 => Self::U64(UInt64Builder::new()),
            ColumnType::F32 => Self::F32(Float32Builder::new()),
            ColumnType::F64 => Self::F64(Float64Builder::new()),
            ColumnType::String => Self::String(StringBuilder::new()),
        }
    }

    fn append_cell(&mut self, value: CellValue, column_name: &str) -> Result<usize> {
        let estimated_bytes = estimate_cell_bytes(&value);
        match (self, value) {
            (Self::Binary(builder), CellValue::Null) => builder.append_null(),
            (Self::Binary(builder), CellValue::Binary(value)) => builder.append_value(value),
            (Self::Bool(builder), CellValue::Null) => builder.append_null(),
            (Self::Bool(builder), CellValue::Bool(value)) => builder.append_value(value),
            (Self::I32(builder), CellValue::Null) => builder.append_null(),
            (Self::I32(builder), CellValue::I32(value)) => builder.append_value(value),
            (Self::I64(builder), CellValue::Null) => builder.append_null(),
            (Self::I64(builder), CellValue::I64(value)) => builder.append_value(value),
            (Self::U32(builder), CellValue::Null) => builder.append_null(),
            (Self::U32(builder), CellValue::U32(value)) => builder.append_value(value),
            (Self::U64(builder), CellValue::Null) => builder.append_null(),
            (Self::U64(builder), CellValue::U64(value)) => builder.append_value(value),
            (Self::F32(builder), CellValue::Null) => builder.append_null(),
            (Self::F32(builder), CellValue::F32(value)) => builder.append_value(value),
            (Self::F64(builder), CellValue::Null) => builder.append_null(),
            (Self::F64(builder), CellValue::F64(value)) => builder.append_value(value),
            (Self::String(builder), CellValue::Null) => builder.append_null(),
            (Self::String(builder), CellValue::String(value)) => builder.append_value(value),
            _ => bail!("column {column_name} received incompatible value"),
        }
        Ok(estimated_bytes)
    }

    fn append_payload_cell(
        &mut self,
        value: &PayloadValue,
        column_type: ColumnType,
        column_name: &str,
    ) -> Result<usize> {
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
            }
            return Ok(CELL_VALUE_FIXED_BYTES);
        }

        match (self, column_type) {
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
            _ => bail!("column {column_name} received incompatible value"),
        }
    }

    fn append_string_value(&mut self, value: Option<&str>, column_name: &str) -> Result<usize> {
        let Self::String(builder) = self else {
            bail!("column {column_name} received incompatible value");
        };
        match value {
            Some(value) => {
                builder.append_value(value);
                Ok(CELL_VALUE_FIXED_BYTES + value.len())
            }
            None => {
                builder.append_null();
                Ok(CELL_VALUE_FIXED_BYTES)
            }
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

fn append_u64_option(builder: &mut UInt64Builder, value: Option<u64>) {
    match value {
        Some(value) => builder.append_value(value),
        None => builder.append_null(),
    }
}

fn arrow_type(column_type: ColumnType) -> DataType {
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
    }
}

fn estimate_cell_bytes(value: &CellValue) -> usize {
    CELL_VALUE_FIXED_BYTES
        + match value {
            CellValue::Null => 0,
            CellValue::Binary(value) => value.len(),
            CellValue::Bool(_) => 1,
            CellValue::I32(_) | CellValue::U32(_) | CellValue::F32(_) => 4,
            CellValue::I64(_) | CellValue::U64(_) | CellValue::F64(_) => 8,
            CellValue::String(value) => value.len(),
        }
}
