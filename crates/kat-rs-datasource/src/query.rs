//! Owns the built DataFusion context and exposes SQL-to-JSON query capability.

use std::{path::Path, sync::Arc};

use anyhow::{Context, Result};
use arrow_array::RecordBatch;
use datafusion::{
    datasource::{MemTable, file_format::file_compression_type::FileCompressionType},
    prelude::{JsonReadOptions, SessionContext},
};
use log::debug;
use serde_json::Value;

use crate::{
    arrow_table::{ArrowTable, ArrowTableSet},
    dataset::register_dataset_tables,
    formats::{hitrace, langfuse},
    json::batches_to_json,
    sinks::arrow::ArrowSink,
};

pub struct TraceDatasource {
    ctx: SessionContext,
}

impl TraceDatasource {
    pub fn from_hitrace(path: impl AsRef<Path>) -> Result<Self> {
        let ctx = SessionContext::new();
        let mut sink = ArrowSink::new()?;
        hitrace::decode_file(path.as_ref(), &mut sink)?;
        register_dataset(&ctx, sink.finish()?)?;

        Ok(Self { ctx })
    }

    pub async fn from_dataset(path: impl AsRef<Path>) -> Result<Self> {
        let ctx = SessionContext::new();
        register_dataset_tables(&ctx, path.as_ref()).await?;

        Ok(Self { ctx })
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

        Ok(Self { ctx })
    }

    pub async fn query_json(&self, sql: &str) -> Result<Value> {
        let batches = self.query_batches(sql).await?;

        batches_to_json(&batches)
    }

    pub async fn query_batches(&self, sql: &str) -> Result<Vec<RecordBatch>> {
        debug!("running datasource sql: {sql}");

        let dataframe = self.ctx.sql(sql).await?;
        dataframe.collect().await.map_err(Into::into)
    }

    pub fn register_record_batches(
        &self,
        table_name: &str,
        batches: Vec<RecordBatch>,
    ) -> Result<()> {
        let schema = batches
            .first()
            .with_context(|| format!("run-local table {table_name} produced no record batches"))?
            .schema();
        let mem_table = MemTable::try_new(schema, vec![batches])?;
        self.ctx.register_table(table_name, Arc::new(mem_table))?;
        Ok(())
    }
}

fn register_dataset(ctx: &SessionContext, dataset: ArrowTableSet) -> Result<()> {
    for table in dataset.tables {
        register_table(ctx, table)?;
    }

    Ok(())
}

fn register_table(ctx: &SessionContext, table: ArrowTable) -> Result<()> {
    let schema = table
        .batches
        .first()
        .with_context(|| format!("datasource table {} is missing batches", table.name))?
        .schema();
    let mem_table = MemTable::try_new(schema, vec![table.batches])?;
    ctx.register_table(table.name, Arc::new(mem_table))?;
    debug!("registered datasource table: {}", table.name);

    Ok(())
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
