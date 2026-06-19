use std::{
    fs::{self, File},
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use datafusion::prelude::SessionContext;
use parquet::arrow::ArrowWriter;
use serde::Serialize;
use tempfile::{Builder, TempDir};

use super::{
    catalog::{DatasetCatalog, DatasetTable},
    reader::register_dataset_tables,
};

pub(crate) struct DatasetWriter {
    target_path: PathBuf,
    temp_dir: TempDir,
    tables: Vec<DatasetTable>,
}

impl DatasetWriter {
    pub(crate) fn create(target_path: &Path) -> Result<Self> {
        let parent = validate_target_path(target_path)?;

        if target_entry_exists(target_path)? {
            bail!("dataset target already exists: {}", target_path.display());
        }

        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create dataset parent directory: {}",
                parent.display()
            )
        })?;

        let target_name = target_path
            .file_name()
            .expect("validated target path has a normal file name")
            .to_string_lossy();
        let temp_prefix = format!("{target_name}.tmp-");
        let temp_dir = Builder::new()
            .prefix(&temp_prefix)
            .tempdir_in(parent)
            .with_context(|| {
                format!(
                    "failed to create dataset temporary directory in {}",
                    parent.display()
                )
            })?;
        let tables_path = temp_dir.path().join("tables");
        fs::create_dir(&tables_path).with_context(|| {
            format!(
                "failed to create dataset tables directory: {}",
                tables_path.display()
            )
        })?;

        Ok(Self {
            target_path: target_path.to_path_buf(),
            temp_dir,
            tables: Vec::new(),
        })
    }

    pub(crate) fn write_batches(
        &mut self,
        logical_name: &str,
        parquet_file_name: &str,
        batches: &[RecordBatch],
    ) -> Result<()> {
        let Some(first_batch) = batches.first() else {
            bail!("dataset table {logical_name} has no record batches");
        };
        let mut table_writer =
            self.start_table(logical_name, parquet_file_name, first_batch.schema())?;

        for batch in batches {
            table_writer.write(batch)?;
        }

        self.add_table(table_writer.finish()?);
        Ok(())
    }

    pub(crate) fn start_table(
        &self,
        logical_name: &str,
        parquet_file_name: &str,
        schema: SchemaRef,
    ) -> Result<DatasetTableWriter> {
        let parquet_path = self.temp_dir.path().join("tables").join(parquet_file_name);
        let file = File::create(&parquet_path).with_context(|| {
            format!("failed to create Parquet table: {}", parquet_path.display())
        })?;
        let writer = ArrowWriter::try_new(file, schema, None)
            .with_context(|| format!("failed to create Parquet writer for {logical_name}"))?;

        Ok(DatasetTableWriter {
            logical_name: logical_name.to_string(),
            relative_path: format!("tables/{parquet_file_name}"),
            writer,
        })
    }

    pub(crate) fn add_table(&mut self, table: DatasetTable) {
        self.tables.push(table);
    }

    pub(crate) async fn finish(self) -> Result<()> {
        write_json(
            &self.temp_dir.path().join("catalog.json"),
            &DatasetCatalog::new(self.tables.clone()),
        )?;

        let ctx = SessionContext::new();
        register_dataset_tables(&ctx, self.temp_dir.path())
            .await
            .with_context(|| {
                format!(
                    "failed to validate temporary dataset: {}",
                    self.temp_dir.path().display()
                )
            })?;

        promote_dataset_write(self)
    }
}

pub(crate) struct DatasetTableWriter {
    logical_name: String,
    relative_path: String,
    writer: ArrowWriter<File>,
}

impl DatasetTableWriter {
    pub(crate) fn write(&mut self, batch: &RecordBatch) -> Result<()> {
        self.writer
            .write(batch)
            .with_context(|| format!("failed to write Parquet table {}", self.logical_name))
    }

    pub(crate) fn finish(self) -> Result<DatasetTable> {
        let Self {
            logical_name,
            relative_path,
            writer,
        } = self;
        writer
            .close()
            .with_context(|| format!("failed to close Parquet table {logical_name}"))?;

        Ok(DatasetTable::new(logical_name, relative_path))
    }
}

fn validate_target_path(target_path: &Path) -> Result<&Path> {
    if target_path.as_os_str().is_empty() {
        bail!("invalid dataset target path: {}", target_path.display());
    }

    if !matches!(
        target_path.components().next_back(),
        Some(Component::Normal(_))
    ) {
        bail!("invalid dataset target path: {}", target_path.display());
    }

    let parent = target_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| anyhow::anyhow!("invalid dataset target path: {}", target_path.display()))?;

    Ok(parent)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let file = File::create(path)
        .with_context(|| format!("failed to create dataset metadata: {}", path.display()))?;
    serde_json::to_writer_pretty(file, value)
        .with_context(|| format!("failed to write dataset metadata: {}", path.display()))?;
    Ok(())
}

fn promote_dataset_write(writer: DatasetWriter) -> Result<()> {
    let temp_path = writer.temp_dir.path().to_path_buf();

    if target_entry_exists(&writer.target_path)? {
        bail!(
            "dataset target already exists: {}",
            writer.target_path.display()
        );
    }

    fs::rename(&temp_path, &writer.target_path).with_context(|| {
        format!(
            "failed to move dataset {} to {}",
            temp_path.display(),
            writer.target_path.display()
        )
    })?;

    Ok(())
}

fn target_entry_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("failed to inspect dataset target: {}", path.display())),
    }
}
