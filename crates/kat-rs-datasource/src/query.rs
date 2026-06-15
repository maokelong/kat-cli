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
    hitrace::{HITRACE_TABLE, load_hitrace_tables},
    json::batches_to_json,
    langfuse::legacy_json_tables,
};

pub struct TraceDatasource {
    ctx: SessionContext,
}

impl TraceDatasource {
    pub fn from_hitrace(path: impl AsRef<Path>) -> Result<Self> {
        let ctx = SessionContext::new();
        let tables = load_hitrace_tables(path.as_ref())?;

        register_batches(
            &ctx,
            HITRACE_TABLE,
            tables.profiler_plugin_data,
            "hitrace file contains no protobuf sections",
        )?;

        for table in tables.tables {
            register_batches(&ctx, table.name, table.batches, "hitrace table is missing")?;
        }

        Ok(Self { ctx })
    }

    pub async fn from_langfuse_legacy(
        observations_path: impl AsRef<Path>,
        traces_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let ctx = SessionContext::new();

        for table in legacy_json_tables(observations_path.as_ref(), traces_path.as_ref()) {
            register_jsonl_gz(&ctx, table.name, table.path).await?;
        }

        Ok(Self { ctx })
    }

    pub async fn query_json(&self, sql: &str) -> Result<Value> {
        debug!("running datasource sql: {sql}");

        let dataframe = self.ctx.sql(sql).await?;
        let batches = dataframe.collect().await?;

        batches_to_json(&batches)
    }
}

fn register_batches(
    ctx: &SessionContext,
    name: &str,
    batches: Vec<RecordBatch>,
    empty_message: &'static str,
) -> Result<()> {
    let schema = batches.first().context(empty_message)?.schema();
    let table = MemTable::try_new(schema, vec![batches])?;
    ctx.register_table(name, Arc::new(table))?;
    debug!("registered datasource table: {name}");

    Ok(())
}

async fn register_jsonl_gz(ctx: &SessionContext, name: &str, path: &Path) -> Result<()> {
    let path = path.to_str().with_context(|| {
        format!(
            "Langfuse export path is not valid UTF-8: {}",
            path.display()
        )
    })?;
    let options = JsonReadOptions::default()
        .file_extension(".jsonl.gz")
        .file_compression_type(FileCompressionType::GZIP);

    ctx.register_json(name, path, options)
        .await
        .with_context(|| format!("failed to register Langfuse JSONL table {name} from {path}"))?;
    debug!("registered datasource table: {name}");

    Ok(())
}
