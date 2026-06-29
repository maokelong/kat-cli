use anyhow::Result;

pub mod sqlite;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetColumn {
    pub name: String,
}

pub trait DatasetAdapter {
    fn table_names(&mut self) -> Result<Vec<String>>;
    fn table_exists(&mut self, table: &str) -> Result<bool>;
    fn table_columns(&mut self, table: &str) -> Result<Vec<DatasetColumn>>;
    fn create_derived_table_as(&mut self, table: &str, sql: &str) -> Result<()>;
}
