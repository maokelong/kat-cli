use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use arrow_array::{Array, ArrayRef, RecordBatch, StructArray, builder::LargeStringBuilder};
use arrow_schema::{DataType, Field, FieldRef, Schema};
use datafusion::{
    datasource::file_format::file_compression_type::FileCompressionType,
    prelude::{JsonReadOptions, SessionContext},
};
use futures::StreamExt;

use crate::{
    arrow_table::ArrowTableSet,
    dataset::DatasetWriter,
    formats::{hitrace, langfuse},
    sinks::arrow::ArrowSink,
};

pub async fn materialize_hitrace_dataset(
    path: impl AsRef<Path>,
    dataset_path: impl AsRef<Path>,
) -> Result<()> {
    let path = path.as_ref();
    let dataset_path = dataset_path.as_ref();

    let mut writer = DatasetWriter::create(dataset_path)?;
    let dataset = decode_hitrace(path)?;
    write_hitrace_tables(&mut writer, dataset)?;
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

fn decode_hitrace(path: &Path) -> Result<ArrowTableSet> {
    let mut sink = ArrowSink::new()?;
    hitrace::decode_file(path, &mut sink)?;
    sink.finish()
}

fn write_hitrace_tables(writer: &mut DatasetWriter, dataset: ArrowTableSet) -> Result<()> {
    for table in dataset.tables {
        writer.write_batches(
            table.name,
            &format!("hitrace.{}.parquet", table.name),
            &table.batches,
        )?;
    }

    Ok(())
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
