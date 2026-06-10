//! Owns the built DataFusion context and exposes SQL-to-JSON query capability.

use std::{path::Path, sync::Arc};

use anyhow::{Context, Result};
use datafusion::{datasource::MemTable, prelude::SessionContext};
use log::debug;
use serde_json::Value;

use crate::{
    hitrace::{HITRACE_TABLE, SCHED_SWITCH_TABLE, load_hitrace_tables},
    json::batches_to_json,
};

pub struct TraceDatasource {
    ctx: SessionContext,
}

impl TraceDatasource {
    pub fn from_hitrace(path: impl AsRef<Path>) -> Result<Self> {
        let ctx = SessionContext::new();
        let tables = load_hitrace_tables(path.as_ref())?;
        let profiler_schema = tables
            .profiler_plugin_data
            .first()
            .context("hitrace file contains no protobuf sections")?
            .schema();
        let profiler_table = MemTable::try_new(profiler_schema, vec![tables.profiler_plugin_data])?;
        ctx.register_table(HITRACE_TABLE, Arc::new(profiler_table))?;
        debug!("registered datasource table: {HITRACE_TABLE}");

        let sched_switch_schema = tables
            .sched_switch
            .first()
            .context("sched_switch table is missing")?
            .schema();
        let sched_switch_table = MemTable::try_new(sched_switch_schema, vec![tables.sched_switch])?;
        ctx.register_table(SCHED_SWITCH_TABLE, Arc::new(sched_switch_table))?;
        debug!("registered datasource table: {SCHED_SWITCH_TABLE}");

        Ok(Self { ctx })
    }

    pub async fn query_json(&self, sql: &str) -> Result<Value> {
        debug!("running datasource sql: {sql}");

        let dataframe = self.ctx.sql(sql).await?;
        let batches = dataframe.collect().await?;

        batches_to_json(&batches)
    }
}
