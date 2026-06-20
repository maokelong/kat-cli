use std::{
    collections::HashSet,
    fs::{self, File},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use datafusion::prelude::{ParquetReadOptions, SessionContext};
use parquet::file::reader::SerializedFileReader;
use url::Url;

use super::catalog::{DatasetCatalog, DatasetTable};

pub(crate) async fn register_dataset_tables(
    ctx: &SessionContext,
    dataset_path: &Path,
) -> Result<()> {
    let tables = validated_dataset_tables(dataset_path)?;

    for table in tables {
        let parquet_url = parquet_file_url(&table.path)?;

        ctx.register_parquet(
            table.name.as_str(),
            parquet_url.as_str(),
            ParquetReadOptions::default(),
        )
        .await
        .with_context(|| format!("failed to register dataset table {}", table.name))?;
    }

    Ok(())
}

fn validated_dataset_tables(dataset_path: &Path) -> Result<Vec<ValidatedDatasetTable>> {
    let catalog = read_catalog(dataset_path)?;
    validate_catalog(&catalog, dataset_path)
}

fn read_catalog(dataset_path: &Path) -> Result<DatasetCatalog> {
    let path = dataset_path.join("catalog.json");
    let json = fs::read_to_string(&path)
        .with_context(|| format!("failed to read dataset catalog: {}", path.display()))?;
    serde_json::from_str(&json)
        .with_context(|| format!("failed to parse dataset catalog: {}", path.display()))
}

struct ValidatedDatasetTable {
    name: String,
    path: PathBuf,
}

fn validate_catalog(
    catalog: &DatasetCatalog,
    dataset_path: &Path,
) -> Result<Vec<ValidatedDatasetTable>> {
    let mut names = HashSet::new();
    let mut tables = Vec::new();
    let dataset_root = dunce::canonicalize(dataset_path).with_context(|| {
        format!(
            "failed to canonicalize dataset path: {}",
            dataset_path.display()
        )
    })?;

    for table in &catalog.tables {
        if !names.insert(table.name.as_str()) {
            bail!("duplicate dataset table name: {}", table.name);
        }
        let path = validate_table_path(table, dataset_path, &dataset_root)?;
        tables.push(ValidatedDatasetTable {
            name: table.name.clone(),
            path,
        });
    }

    Ok(tables)
}

fn validate_table_path(
    table: &DatasetTable,
    dataset_path: &Path,
    dataset_root: &Path,
) -> Result<PathBuf> {
    let relative_path = Path::new(&table.path);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!(
            "dataset table {} has invalid relative path: {}",
            table.name,
            table.path
        );
    }

    let parquet_path = dataset_path.join(relative_path);
    if !parquet_path.exists() {
        bail!(
            "dataset table {} references missing Parquet file: {}",
            table.name,
            parquet_path.display()
        );
    }

    let parquet_path = dunce::canonicalize(&parquet_path).with_context(|| {
        format!(
            "failed to canonicalize dataset table path: {}",
            parquet_path.display()
        )
    })?;
    if !parquet_path.starts_with(dataset_root) {
        bail!(
            "dataset table {} path escapes dataset directory: {}",
            table.name,
            table.path
        );
    }

    validate_parquet_metadata(table, &parquet_path)?;

    Ok(parquet_path)
}

fn validate_parquet_metadata(table: &DatasetTable, parquet_path: &Path) -> Result<()> {
    let file = File::open(parquet_path).with_context(|| {
        format!(
            "failed to open dataset table Parquet file for {}: {}",
            table.name,
            parquet_path.display()
        )
    })?;
    drop(SerializedFileReader::new(file).with_context(|| {
        format!(
            "failed to read dataset table Parquet metadata for {}: {}",
            table.name,
            parquet_path.display()
        )
    })?);

    Ok(())
}

fn parquet_file_url(path: &Path) -> Result<String> {
    let url = Url::from_file_path(path).map_err(|()| {
        anyhow!(
            "dataset table path cannot be converted to file URL: {}",
            path.display()
        )
    })?;

    Ok(url.to_string())
}
