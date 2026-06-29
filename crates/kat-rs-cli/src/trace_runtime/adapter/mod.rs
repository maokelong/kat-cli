use anyhow::Result;
use serde_json::Value;

pub mod sqlite;

pub trait DatasetAdapter {
    fn table_names(&mut self) -> Result<Vec<String>>;
    fn table_exists(&mut self, table: &str) -> Result<bool>;
    fn query_json(&mut self, sql: &str) -> Result<Vec<Value>>;
    fn create_derived_table_as(&mut self, table: &str, sql: &str) -> Result<()>;
}
