use std::{
    fs::{self, File},
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use datafusion::prelude::SessionContext;
use parquet::{arrow::ArrowWriter, file::properties::WriterProperties};
use serde::Serialize;
use tempfile::{Builder, TempDir};

use super::{
    catalog::{DatasetCatalog, DatasetTable},
    reader::register_dataset_tables,
};

const DATASET_PARQUET_MAX_ROW_GROUP_ROWS: usize = 64 * 1024;
const DATASET_PARQUET_MAX_ROW_GROUP_BYTES: usize = 64 * 1024 * 1024;

pub async fn write_derived_dataset_table(
    dataset_path: &Path,
    logical_name: &str,
    pack_ref: &str,
    transform_id: &str,
    batches: &[RecordBatch],
) -> Result<()> {
    let Some(first_batch) = batches.first() else {
        bail!("derived dataset table {logical_name} has no record batches");
    };
    validate_file_component(logical_name, "derived table name")?;
    validate_file_component(pack_ref, "packRef")?;
    validate_file_component(transform_id, "transformId")?;

    let mut catalog = read_catalog(dataset_path)?;
    if catalog
        .tables
        .iter()
        .any(|table| table.name == logical_name)
    {
        bail!("dataset table already exists: {logical_name}");
    }

    let derived_dir = dataset_path.join("derived").join(pack_ref);
    fs::create_dir_all(&derived_dir).with_context(|| {
        format!(
            "failed to create derived dataset directory: {}",
            derived_dir.display()
        )
    })?;

    let parquet_file_name = format!("{transform_id}.{logical_name}.parquet");
    let parquet_path = derived_dir.join(&parquet_file_name);
    let file = File::create(&parquet_path).with_context(|| {
        format!(
            "failed to create derived Parquet table: {}",
            parquet_path.display()
        )
    })?;
    let mut writer = ArrowWriter::try_new(
        file,
        first_batch.schema(),
        Some(dataset_parquet_writer_properties()),
    )
    .with_context(|| format!("failed to create Parquet writer for {logical_name}"))?;

    for batch in batches {
        writer
            .write(batch)
            .with_context(|| format!("failed to write derived Parquet table {logical_name}"))?;
    }
    writer
        .close()
        .with_context(|| format!("failed to close derived Parquet table {logical_name}"))?;

    let relative_path = format!("derived/{pack_ref}/{parquet_file_name}");
    catalog
        .tables
        .push(DatasetTable::parquet(logical_name, relative_path));
    write_json(&dataset_path.join("catalog.json"), &catalog)?;

    let ctx = SessionContext::new();
    register_dataset_tables(&ctx, dataset_path)
        .await
        .with_context(|| {
            format!(
                "failed to validate dataset after derived table write: {}",
                dataset_path.display()
            )
        })?;

    Ok(())
}

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
        let writer = ArrowWriter::try_new(file, schema, Some(dataset_parquet_writer_properties()))
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
        File::create(self.temp_dir.path().join(".kat-dataset"))
            .context("failed to create dataset marker")?;
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

fn dataset_parquet_writer_properties() -> WriterProperties {
    WriterProperties::builder()
        .set_max_row_group_row_count(Some(DATASET_PARQUET_MAX_ROW_GROUP_ROWS))
        .set_max_row_group_bytes(Some(DATASET_PARQUET_MAX_ROW_GROUP_BYTES))
        .build()
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

        Ok(DatasetTable::parquet(logical_name, relative_path))
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

fn read_catalog(dataset_path: &Path) -> Result<DatasetCatalog> {
    let path = dataset_path.join("catalog.json");
    let json = fs::read_to_string(&path)
        .with_context(|| format!("failed to read dataset catalog: {}", path.display()))?;
    serde_json::from_str(&json)
        .with_context(|| format!("failed to parse dataset catalog: {}", path.display()))
}

fn validate_file_component(value: &str, label: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{label} must not be empty");
    }

    let path = Path::new(value);
    if !matches!(path.components().next(), Some(Component::Normal(_)))
        || path.components().count() != 1
    {
        bail!("{label} must be a single path component: {value}");
    }

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
