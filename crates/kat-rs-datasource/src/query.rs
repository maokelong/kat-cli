//! Owns the built DataFusion context and exposes SQL-to-JSON query capability.

use std::sync::Arc;

use anyhow::Result;
use datafusion::{datasource::MemTable, prelude::SessionContext};
use log::debug;
use serde_json::Value;

use crate::{
    config::{DataSourceConfig, DataSourceType},
    hitrace::{HITRACE_TABLE, load_hitrace_batch},
    json::batches_to_json,
};

pub struct TraceDatasource {
    ctx: SessionContext,
}

impl TraceDatasource {
    pub fn build(config: DataSourceConfig) -> Result<Self> {
        let ctx = SessionContext::new();

        match config.source_type {
            DataSourceType::Hitrace => {
                let batch = load_hitrace_batch(&config.path)?;
                let table = MemTable::try_new(batch.schema(), vec![vec![batch]])?;
                ctx.register_table(HITRACE_TABLE, Arc::new(table))?;
                debug!("registered datasource table: {HITRACE_TABLE}");
            }
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
