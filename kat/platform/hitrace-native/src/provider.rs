use std::{fs::File, path::Path, sync::Arc};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use datafusion_catalog::{MemorySchemaProvider, SchemaProvider, TableProvider};
use datafusion_catalog_listing::{ListingOptions, ListingTable, ListingTableConfig};
use datafusion_common::Result as DataFusionResult;
use datafusion_datasource::ListingTableUrl;
use datafusion_datasource_parquet::file_format::ParquetFormat;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use tempfile::TempDir;
use url::Url;

const REQUIRED_TABLES: [&str; 2] = ["clock_domain", "clock_snapshot"];
const OPTIONAL_TABLES: [&str; 1] = ["sched_switch"];

#[derive(Debug)]
pub(crate) struct HitraceSchema {
    _staging: TempDir,
    inner: MemorySchemaProvider,
}

impl HitraceSchema {
    pub(crate) fn open(trace: &Path) -> Result<Self> {
        let staging = tempfile::Builder::new()
            .prefix("kat-hitrace-source-")
            .tempdir()
            .context("failed to create private Hitrace Source staging")?;
        let staged =
            kat_datasource::stage_hitrace(trace, staging.path().join("hitrace"), |_| Ok(()))
                .map_err(anyhow::Error::new)
                .with_context(|| {
                    format!(
                        "failed to create Hitrace Source provider from {}",
                        trace.display()
                    )
                })?;

        let inner = MemorySchemaProvider::new();
        for name in REQUIRED_TABLES {
            let path = staged
                .table_names()
                .iter()
                .find(|table| table.as_str() == name)
                .map(|_| staged.tables_directory().join(format!("{name}.parquet")))
                .with_context(|| {
                    format!("Hitrace staging did not produce required table {name}")
                })?;
            inner
                .register_table(name.to_owned(), parquet_table(&path)?)
                .with_context(|| format!("failed to register Hitrace table {name}"))?;
        }
        for name in OPTIONAL_TABLES {
            if let Some(path) = staged
                .table_names()
                .iter()
                .find(|table| table.as_str() == name)
                .map(|_| staged.tables_directory().join(format!("{name}.parquet")))
            {
                inner
                    .register_table(name.to_owned(), parquet_table(&path)?)
                    .with_context(|| format!("failed to register Hitrace table {name}"))?;
            }
        }

        Ok(Self {
            _staging: staging,
            inner,
        })
    }

    #[cfg(test)]
    fn staging_path(&self) -> &Path {
        self._staging.path()
    }
}

fn parquet_table(path: &Path) -> Result<Arc<dyn TableProvider>> {
    let file = File::open(path)
        .with_context(|| format!("failed to open Hitrace table {}", path.display()))?;
    let schema = ParquetRecordBatchReaderBuilder::try_new(file)
        .with_context(|| format!("failed to read Hitrace table metadata {}", path.display()))?
        .schema()
        .clone();
    let url = Url::from_file_path(path).map_err(|()| {
        anyhow!(
            "failed to convert Hitrace table path to a file URL: {}",
            path.display()
        )
    })?;
    let table_url = ListingTableUrl::try_new(url, None)?;
    let options =
        ListingOptions::new(Arc::new(ParquetFormat::new())).with_file_extension(".parquet");
    let config = ListingTableConfig::new(table_url)
        .with_listing_options(options)
        .with_schema(schema);
    Ok(Arc::new(ListingTable::try_new(config)?))
}

#[async_trait]
impl SchemaProvider for HitraceSchema {
    fn table_names(&self) -> Vec<String> {
        let mut names = self.inner.table_names();
        names.sort();
        names
    }

    async fn table(&self, name: &str) -> DataFusionResult<Option<Arc<dyn TableProvider>>> {
        self.inner.table(name).await
    }

    fn table_exist(&self, name: &str) -> bool {
        self.inner.table_exist(name)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

    use anyhow::Result;
    use arrow_array::Int64Array;
    use datafusion::prelude::SessionContext;
    use datafusion_catalog::SchemaProvider;

    use super::HitraceSchema;

    const HEADER_SIZE: usize = 1024;
    const HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;

    fn write_header_only_trace(path: &Path) -> Result<()> {
        let mut data = vec![0; HEADER_SIZE];
        data[0..8].copy_from_slice(&HEADER_MAGIC.to_le_bytes());
        data[8..16].copy_from_slice(&(HEADER_SIZE as u64).to_le_bytes());
        for (offset, value) in [60, 68, 76, 84, 92, 100].into_iter().zip(1_u64..=6) {
            data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        fs::write(path, data)?;
        Ok(())
    }

    #[tokio::test]
    async fn staged_tables_are_stable_and_queryable_after_the_trace_is_removed() -> Result<()> {
        let root = tempfile::tempdir()?;
        let trace = root.path().join("capture.htrace");
        write_header_only_trace(&trace)?;

        let provider = HitraceSchema::open(&trace)?;
        let staging = provider.staging_path().to_owned();
        fs::remove_file(&trace)?;

        assert_eq!(provider.table_names(), ["clock_domain", "clock_snapshot"]);
        assert!(!provider.table_exist("sched_switch"));
        let first = provider.table("clock_domain").await?.unwrap();
        let second = provider.table("clock_domain").await?.unwrap();
        assert!(Arc::ptr_eq(&first, &second));

        let ctx = SessionContext::new();
        ctx.register_table("clock_domain", first)?;
        for _ in 0..2 {
            let batches = ctx
                .sql("SELECT COUNT(*) AS count FROM clock_domain")
                .await?
                .collect()
                .await?;
            let counts = batches[0]
                .column(0)
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap();
            assert_eq!(counts.value(0), 6);
        }

        drop(second);
        drop(ctx);
        drop(provider);
        assert!(!staging.exists());
        Ok(())
    }
}
