//! Owns the built DataFusion context and exposes SQL-to-JSON query capability.

use std::{path::Path, sync::Arc};

use anyhow::{Context, Result};
use datafusion::{datasource::MemTable, prelude::SessionContext};
use log::debug;
use serde_json::Value;

use crate::{
    catalog::{TraceDataset, TraceTable},
    formats::hitrace,
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

    pub async fn query_json(&self, sql: &str) -> Result<Value> {
        debug!("running datasource sql: {sql}");

        let dataframe = self.ctx.sql(sql).await?;
        let batches = dataframe.collect().await?;

        batches_to_json(&batches)
    }
}

fn register_dataset(ctx: &SessionContext, dataset: TraceDataset) -> Result<()> {
    for table in dataset.tables {
        register_table(ctx, table)?;
    }

    Ok(())
}

fn register_table(ctx: &SessionContext, table: TraceTable) -> Result<()> {
    let schema = table
        .batches
        .first()
        .with_context(|| format!("datasource table {} is missing batches", table.name))?
        .schema();
    let mem_table = MemTable::try_new(schema, vec![table.batches])?;
    ctx.register_table(table.name, Arc::new(mem_table))?;
    debug!(
        "registered datasource table: {} category={:?}",
        table.name, table.category
    );

    Ok(())
}
