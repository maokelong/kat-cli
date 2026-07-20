use std::{
    io,
    path::{Path, PathBuf},
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
    dataset::DatasetWriter,
    dataset_writer::DatasetWriteTarget,
    formats::{hitrace, langfuse},
    relational::sink::RelationalDatasetSink,
};

#[derive(Debug)]
pub struct ImportedHitrace {
    path: PathBuf,
    unsupported_plugins: Vec<String>,
    unsupported_section_types: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct UnsupportedHitraceContent {
    kind: &'static str,
    value: String,
    byte_offset: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum HitraceImportError {
    #[error("{source}")]
    Import {
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to report unsupported Hitrace content")]
    ObserveUnsupportedContent {
        #[source]
        source: io::Error,
    },
}

impl ImportedHitrace {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn unsupported_plugins(&self) -> &[String] {
        &self.unsupported_plugins
    }

    pub fn unsupported_section_types(&self) -> &[u32] {
        &self.unsupported_section_types
    }
}

impl UnsupportedHitraceContent {
    pub fn kind(&self) -> &str {
        self.kind
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn byte_offset(&self) -> usize {
        self.byte_offset
    }
}

impl HitraceImportError {
    fn import(source: anyhow::Error) -> Self {
        Self::Import { source }
    }
}

pub fn import_hitrace(
    path: impl AsRef<Path>,
    target: DatasetWriteTarget,
    mut observe_unsupported: impl FnMut(&UnsupportedHitraceContent) -> io::Result<()>,
) -> std::result::Result<ImportedHitrace, HitraceImportError> {
    let path = path.as_ref();
    let dataset_path = target
        .prepare_for_relational_write()
        .map_err(|source| HitraceImportError::import(source.into()))?;
    let mut observer_failure = None;
    let mut observe = |content: &hitrace::UnsupportedHitraceContent| {
        let content = UnsupportedHitraceContent {
            kind: content.kind,
            value: content.value.clone(),
            byte_offset: content.byte_offset,
        };
        if let Err(source) = observe_unsupported(&content) {
            observer_failure = Some(source);
            bail!("unsupported Hitrace content observer failed");
        }
        Ok(())
    };

    let result = futures::executor::block_on(materialize_hitrace_dataset_with_report(
        path,
        &dataset_path,
        &mut observe,
    ));
    if let Some(source) = observer_failure {
        return Err(HitraceImportError::ObserveUnsupportedContent { source });
    }
    let report = result.map_err(HitraceImportError::import)?;

    Ok(ImportedHitrace {
        path: dataset_path,
        unsupported_plugins: report.unsupported_plugins.into_iter().collect(),
        unsupported_section_types: report.unsupported_section_types.into_iter().collect(),
    })
}

pub async fn materialize_hitrace_dataset(
    path: impl AsRef<Path>,
    dataset_path: impl AsRef<Path>,
) -> Result<()> {
    materialize_hitrace_dataset_with_report(path.as_ref(), dataset_path.as_ref(), &mut |_| Ok(()))
        .await?;
    Ok(())
}

async fn materialize_hitrace_dataset_with_report(
    path: &Path,
    dataset_path: &Path,
    observe_unsupported: &mut impl FnMut(&hitrace::UnsupportedHitraceContent) -> Result<()>,
) -> Result<hitrace::HitraceDecodeReport> {
    let writer = DatasetWriter::create(dataset_path)?;
    let mut sink = RelationalDatasetSink::new(writer)?;
    let report = hitrace::decode_file_with_report(path, &mut sink, observe_unsupported)
        .map_err(|failure| failure.source)
        .with_context(|| format!("failed to decode hitrace file: {}", path.display()))?;
    let writer = sink.finish()?;
    writer.finish().await?;
    Ok(report)
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
