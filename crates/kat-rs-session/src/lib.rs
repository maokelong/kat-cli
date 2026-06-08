//! Stores session state shared by CLI commands and future interactive callers.

use anyhow::{Result, bail};
use kat_rs_datasource::{DataSourceConfig, TraceDatasource};
use serde_json::Value;

pub struct Session {
    datasource: Option<TraceDatasource>,
}

impl Session {
    pub fn create() -> Self {
        Self { datasource: None }
    }

    pub fn build_datasource(&mut self, config: DataSourceConfig) -> Result<()> {
        self.datasource = Some(TraceDatasource::build(config)?);
        Ok(())
    }

    pub async fn query_json(&self, sql: &str) -> Result<Value> {
        let Some(datasource) = &self.datasource else {
            bail!("datasource is not built");
        };

        datasource.query_json(sql).await
    }
}
