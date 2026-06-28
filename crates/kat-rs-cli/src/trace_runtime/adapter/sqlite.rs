use std::path::Path;

use anyhow::{Result, bail};
use rusqlite::{Connection, Row, types::ValueRef};
use serde_json::{Map, Value, json};

use super::DatasetAdapter;

pub struct SQLiteDatasetAdapter {
    conn: Connection,
}

impl SQLiteDatasetAdapter {
    pub fn open(raw_db: impl AsRef<Path>, scratch_db: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(scratch_db)?;
        let raw_db = raw_db.as_ref().to_string_lossy().replace('\'', "''");
        conn.execute(&format!("ATTACH DATABASE '{raw_db}' AS raw"), [])?;
        let mut adapter = Self { conn };
        adapter.install_raw_table_views()?;
        Ok(adapter)
    }

    pub fn query_json(&mut self, sql: &str) -> Result<Vec<Value>> {
        <Self as DatasetAdapter>::query_json(self, sql)
    }

    fn install_raw_table_views(&mut self) -> Result<()> {
        let tables = self.raw_table_names()?;
        for table in tables {
            let view = quote_identifier(&table)?;
            let raw = quote_qualified("raw", &table)?;
            self.conn.execute(
                &format!("CREATE TEMP VIEW {view} AS SELECT * FROM {raw}"),
                [],
            )?;
        }
        Ok(())
    }

    fn raw_table_names(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT name FROM raw.sqlite_master
             WHERE type IN ('table', 'view')
               AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
             ORDER BY name",
        )?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

impl DatasetAdapter for SQLiteDatasetAdapter {
    fn table_names(&mut self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT name FROM sqlite_temp_master WHERE type='view'
             UNION
             SELECT name FROM sqlite_master WHERE type='table'
             ORDER BY name",
        )?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn table_exists(&mut self, table: &str) -> Result<bool> {
        Ok(self.table_names()?.iter().any(|name| name == table))
    }

    fn query_json(&mut self, sql: &str) -> Result<Vec<Value>> {
        let mut stmt = self.conn.prepare(sql)?;
        let column_names = stmt
            .column_names()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let rows = stmt
            .query_map([], |row| row_to_json(row, &column_names))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn create_derived_table_as(&mut self, table: &str, sql: &str) -> Result<()> {
        if self.table_exists(table)? {
            bail!("output table already exists: {table}");
        }
        let table = quote_identifier(table)?;
        self.conn
            .execute(&format!("CREATE TABLE {table} AS {sql}"), [])?;
        Ok(())
    }
}

fn quote_qualified(schema: &str, table: &str) -> Result<String> {
    Ok(format!(
        "{}.{}",
        quote_identifier(schema)?,
        quote_identifier(table)?
    ))
}

fn quote_identifier(identifier: &str) -> Result<String> {
    if identifier.is_empty()
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        bail!("unsafe sqlite identifier: {identifier}");
    }
    Ok(format!("\"{identifier}\""))
}

fn row_to_json(row: &Row<'_>, column_names: &[String]) -> rusqlite::Result<Value> {
    let mut object = Map::new();
    for (index, column_name) in column_names.iter().enumerate() {
        let value = match row.get_ref(index)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(value) => json!(value),
            ValueRef::Real(value) => json!(value),
            ValueRef::Text(value) => Value::String(String::from_utf8_lossy(value).into_owned()),
            ValueRef::Blob(value) => Value::String(format!("<blob:{} bytes>", value.len())),
        };
        object.insert(column_name.clone(), value);
    }
    Ok(Value::Object(object))
}
