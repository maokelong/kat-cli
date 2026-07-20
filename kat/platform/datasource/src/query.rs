//! Owns the built DataFusion context and exposes SQL-to-JSON query capability.

use std::{path::Path, sync::Arc};

use anyhow::{Context, Result};
use datafusion::{
    datasource::{MemTable, file_format::file_compression_type::FileCompressionType},
    prelude::{JsonReadOptions, SessionContext},
};
use log::debug;
use serde_json::Value;
use tempfile::TempDir;

use crate::{
    dataset::register_dataset_tables, formats::langfuse, json::batches_to_json,
    materialize_hitrace_dataset,
};

pub struct TraceDatasource {
    ctx: SessionContext,
    _temp_dataset: Option<TempDir>,
}

impl TraceDatasource {
    pub fn from_hitrace(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let temp_dataset =
            tempfile::tempdir().context("failed to create hitrace query workspace")?;
        let dataset_path = temp_dataset.path().join("dataset");

        futures::executor::block_on(materialize_hitrace_dataset(path, &dataset_path))?;
        let ctx = SessionContext::new();
        futures::executor::block_on(register_dataset_tables(&ctx, &dataset_path))?;

        Ok(Self {
            ctx,
            _temp_dataset: Some(temp_dataset),
        })
    }

    pub async fn from_dataset(path: impl AsRef<Path>) -> Result<Self> {
        let ctx = SessionContext::new();
        register_dataset_tables(&ctx, path.as_ref()).await?;

        Ok(Self {
            ctx,
            _temp_dataset: None,
        })
    }

    pub async fn from_langfuse_legacy(
        observations_path: impl AsRef<Path>,
        traces_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let ctx = SessionContext::new();

        for table in langfuse::legacy_json_tables(observations_path.as_ref(), traces_path.as_ref())
        {
            register_materialized_jsonl_gz(&ctx, table.name, table.path).await?;
        }

        Ok(Self {
            ctx,
            _temp_dataset: None,
        })
    }

    pub async fn query_json(&self, sql: &str) -> Result<Value> {
        debug!("running datasource sql: {sql}");

        let dataframe = self.ctx.sql(sql).await?;
        let batches = dataframe.collect().await?;

        batches_to_json(&batches)
    }
}

async fn register_materialized_jsonl_gz(
    ctx: &SessionContext,
    name: &str,
    path: &Path,
) -> Result<()> {
    let path = path.to_str().with_context(|| {
        format!(
            "Langfuse export path is not valid UTF-8: {}",
            path.display()
        )
    })?;
    let staging_ctx = SessionContext::new();
    let options = JsonReadOptions::default()
        .file_extension(".jsonl.gz")
        .file_compression_type(FileCompressionType::GZIP);

    staging_ctx
        .register_json(name, path, options)
        .await
        .with_context(|| format!("failed to register Langfuse JSONL table {name} from {path}"))?;
    let dataframe = staging_ctx
        .table(name)
        .await
        .with_context(|| format!("failed to read Langfuse JSONL table {name} from {path}"))?;
    let batches = dataframe.collect().await.with_context(|| {
        format!("failed to materialize Langfuse JSONL table {name} from {path}")
    })?;
    let schema = batches
        .first()
        .with_context(|| format!("Langfuse JSONL table {name} from {path} produced no batches"))?
        .schema();
    let mem_table = MemTable::try_new(schema, vec![batches])?;

    ctx.register_table(name, Arc::new(mem_table))?;
    debug!("registered materialized datasource table: {name}");

    Ok(())
}
