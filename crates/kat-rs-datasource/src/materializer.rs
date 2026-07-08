use std::{
    path::{Component, Path},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use arrow_array::{Array, ArrayRef, RecordBatch, StructArray, builder::LargeStringBuilder};
use arrow_schema::{DataType, Field, FieldRef, Schema};
use datafusion::{
    datasource::file_format::file_compression_type::FileCompressionType,
    prelude::{JsonReadOptions, SessionContext},
};
use futures::StreamExt;

use crate::{
    arrow_table::ArrowTable,
    dataset::{DatasetTableWriter, DatasetWriter},
    formats::{hitrace, langfuse, sqlite},
    record::{TraceRecord, TraceRecordSink},
    sinks::arrow::ArrowSink,
};

const HITRACE_DATASET_FLUSH_RECORDS: usize = 64 * 1024;
const SQLITE_DATASET_BATCH_ROWS: usize = 8192;

pub async fn materialize_hitrace_dataset(
    path: impl AsRef<Path>,
    dataset_path: impl AsRef<Path>,
) -> Result<()> {
    let path = path.as_ref();
    let dataset_path = dataset_path.as_ref();

    let writer = DatasetWriter::create(dataset_path)?;
    let mut sink = HitraceDatasetSink::new(writer)?;
    hitrace::decode_file(path, &mut sink)
        .with_context(|| format!("failed to decode hitrace file: {}", path.display()))?;
    let writer = sink.finish()?;
    writer.finish().await
}

pub async fn materialize_langfuse_legacy_dataset(
    observations_path: impl AsRef<Path>,
    traces_path: impl AsRef<Path>,
    dataset_path: impl AsRef<Path>,
) -> Result<()> {
    let observations_path = observations_path.as_ref();
    let traces_path = traces_path.as_ref();
    let dataset_path = dataset_path.as_ref();

    let mut writer = DatasetWriter::create(dataset_path)?;
    write_langfuse_tables(&mut writer, observations_path, traces_path)
        .await
        .with_context(|| {
            format!(
                "failed to write Langfuse legacy dataset tables: {}",
                dataset_path.display()
            )
        })?;
    writer.finish().await
}

pub async fn materialize_sqlite_dataset(
    path: impl AsRef<Path>,
    dataset_path: impl AsRef<Path>,
) -> Result<()> {
    let path = path.as_ref();
    let dataset_path = dataset_path.as_ref();

    let mut writer = DatasetWriter::create(dataset_path)?;
    write_sqlite_tables(&mut writer, path).with_context(|| {
        format!(
            "failed to write SQLite dataset tables: {}",
            dataset_path.display()
        )
    })?;
    writer.finish().await
}

async fn write_langfuse_tables(
    writer: &mut DatasetWriter,
    observations_path: &Path,
    traces_path: &Path,
) -> Result<()> {
    for table in langfuse::legacy_json_tables(observations_path, traces_path) {
        write_langfuse_table(writer, table.name, table.path).await?;
    }

    Ok(())
}

fn write_sqlite_tables(writer: &mut DatasetWriter, path: &Path) -> Result<()> {
    let conn = sqlite::open(path)?;

    for object in sqlite::objects(&conn)? {
        let parquet_file_name = sqlite_parquet_file_name(&object.name)?;
        let mut table_writer =
            writer.start_table(&object.name, &parquet_file_name, sqlite::schema(&object)?)?;
        sqlite::stream_object(&conn, &object, &mut table_writer, SQLITE_DATASET_BATCH_ROWS)?;
        writer.add_table(table_writer.finish()?);
    }

    Ok(())
}

fn sqlite_parquet_file_name(object_name: &str) -> Result<String> {
    let file_name = format!("sqlite.{object_name}.parquet");
    let path = Path::new(&file_name);
    if !matches!(path.components().next(), Some(Component::Normal(_)))
        || path.components().count() != 1
    {
        bail!("SQLite object name must be a single path component: {object_name}");
    }

    Ok(file_name)
}

struct HitraceDatasetSink {
    arrow_sink: ArrowSink,
    dataset_writer: DatasetWriter,
    table_writers: Vec<OpenHitraceTableWriter>,
    records_since_flush: usize,
}

struct OpenHitraceTableWriter {
    name: &'static str,
    writer: DatasetTableWriter,
}

impl HitraceDatasetSink {
    fn new(dataset_writer: DatasetWriter) -> Result<Self> {
        Ok(Self {
            arrow_sink: ArrowSink::new()?,
            dataset_writer,
            table_writers: Vec::new(),
            records_since_flush: 0,
        })
    }

    fn finish(mut self) -> Result<DatasetWriter> {
        self.flush_tables(true)?;

        for table in self.table_writers {
            self.dataset_writer.add_table(table.writer.finish()?);
        }

        Ok(self.dataset_writer)
    }

    fn flush_tables(&mut self, include_empty_tables: bool) -> Result<()> {
        let tables = self.arrow_sink.flush()?;

        for table in tables.tables {
            let row_count = table_row_count(&table);
            if row_count == 0 {
                let already_open = self
                    .table_writers
                    .iter()
                    .any(|open_table| open_table.name == table.name);
                if already_open || !include_empty_tables {
                    continue;
                }
            }

            let writer = self.table_writer_for(&table)?;
            for batch in &table.batches {
                writer.write(batch)?;
            }
        }

        self.records_since_flush = 0;
        Ok(())
    }

    fn table_writer_for(&mut self, table: &ArrowTable) -> Result<&mut DatasetTableWriter> {
        if let Some(index) = self
            .table_writers
            .iter()
            .position(|open_table| open_table.name == table.name)
        {
            return Ok(&mut self.table_writers[index].writer);
        }

        let first_batch = table
            .batches
            .first()
            .with_context(|| format!("hitrace table {} has no record batches", table.name))?;
        let parquet_file_name = format!("hitrace.{}.parquet", table.name);
        let writer = self.dataset_writer.start_table(
            table.name,
            &parquet_file_name,
            first_batch.schema(),
        )?;
        self.table_writers.push(OpenHitraceTableWriter {
            name: table.name,
            writer,
        });
        let index = self.table_writers.len() - 1;

        Ok(&mut self.table_writers[index].writer)
    }
}

impl TraceRecordSink for HitraceDatasetSink {
    fn push(&mut self, record: TraceRecord) -> Result<()> {
        self.arrow_sink.push(record)?;
        self.records_since_flush += 1;

        if self.records_since_flush >= HITRACE_DATASET_FLUSH_RECORDS {
            self.flush_tables(false)?;
        }

        Ok(())
    }
}

fn table_row_count(table: &ArrowTable) -> usize {
    table
        .batches
        .iter()
        .map(RecordBatch::num_rows)
        .sum::<usize>()
}

async fn write_langfuse_table(
    dataset_writer: &mut DatasetWriter,
    table_name: &str,
    jsonl_path: &Path,
) -> Result<()> {
    let jsonl_path_str = jsonl_path.to_str().with_context(|| {
        format!(
            "Langfuse export path is not valid UTF-8: {}",
            jsonl_path.display()
        )
    })?;
    let staging_ctx = SessionContext::new();
    // Keep parity with the legacy datasource's DataFusion JSON inference; explicit schema is future work.
    let options = JsonReadOptions::default()
        .file_extension(".jsonl.gz")
        .file_compression_type(FileCompressionType::GZIP);

    staging_ctx
        .register_json(table_name, jsonl_path_str, options)
        .await
        .with_context(|| {
            format!("failed to register Langfuse JSONL table {table_name} from {jsonl_path_str}")
        })?;
    let dataframe = staging_ctx.table(table_name).await.with_context(|| {
        format!("failed to read Langfuse JSONL table {table_name} from {jsonl_path_str}")
    })?;
    let mut stream = dataframe.execute_stream().await.with_context(|| {
        format!("failed to stream Langfuse JSONL table {table_name} from {jsonl_path_str}")
    })?;

    let parquet_file_name = format!("langfuse.{table_name}.parquet");
    let mut parquet_writer = None;

    while let Some(batch) = stream.next().await {
        let batch = batch.with_context(|| {
            format!("failed to stream Langfuse JSONL table {table_name} from {jsonl_path_str}")
        })?;
        let batch = parquet_compatible_langfuse_batch(batch)?;

        if parquet_writer.is_none() {
            parquet_writer =
                Some(dataset_writer.start_table(table_name, &parquet_file_name, batch.schema())?);
        }

        parquet_writer
            .as_mut()
            .expect("writer is initialized before writing batches")
            .write(&batch)?;
    }

    let Some(parquet_writer) = parquet_writer else {
        bail!("Langfuse JSONL table {table_name} from {jsonl_path_str} produced no batches");
    };
    dataset_writer.add_table(parquet_writer.finish()?);

    Ok(())
}

fn parquet_compatible_langfuse_batch(batch: RecordBatch) -> Result<RecordBatch> {
    let schema = batch.schema();
    let mut fields = Vec::with_capacity(schema.fields().len());
    let mut columns = Vec::with_capacity(batch.num_columns());
    let mut changed = false;

    for (field, column) in schema.fields().iter().zip(batch.columns()) {
        if matches!(field.data_type(), DataType::Struct(fields) if fields.is_empty()) {
            changed = true;
            fields.push(langfuse_json_string_field(field));
            columns.push(empty_struct_column_to_json(column.as_ref())?);
        } else {
            fields.push(Arc::clone(field));
            columns.push(Arc::clone(column));
        }
    }

    if !changed {
        return Ok(batch);
    }

    let schema = Schema::new_with_metadata(fields, schema.metadata().clone());
    RecordBatch::try_new(Arc::new(schema), columns)
        .context("failed to convert Langfuse empty object columns before Parquet write")
}

fn langfuse_json_string_field(field: &FieldRef) -> FieldRef {
    let mut converted = Field::new(field.name(), DataType::LargeUtf8, field.is_nullable());
    converted.set_metadata(field.metadata().clone());
    Arc::new(converted)
}

fn empty_struct_column_to_json(column: &dyn Array) -> Result<ArrayRef> {
    let struct_column = column
        .as_any()
        .downcast_ref::<StructArray>()
        .context("Langfuse empty object column was not an Arrow struct array")?;
    let mut builder = LargeStringBuilder::with_capacity(column.len(), column.len() * 2);

    for row in 0..column.len() {
        if struct_column.is_null(row) {
            builder.append_null();
        } else {
            builder.append_value("{}");
        }
    }

    Ok(Arc::new(builder.finish()))
}
