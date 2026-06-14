use std::path::Path;

use anyhow::{Result, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, types::ValueRef};
use serde_json::{Map, Value, json};

use crate::trace_runtime::query_client::{QueryClient, QueryWindowMode, QueryWindowRequest};

pub struct SqliteQueryClient {
    conn: Connection,
}

impl SqliteQueryClient {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

        Ok(Self { conn })
    }

    fn query_rows(&mut self, sql: &str) -> Result<Vec<Value>> {
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

    fn metadata_rows(&mut self, limit: Option<u32>) -> Result<Vec<Value>> {
        let mut sql =
            "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY name"
                .to_string();
        if let Some(limit) = limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }

        let tables = {
            let mut stmt = self.conn.prepare(&sql)?;
            stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };

        tables
            .into_iter()
            .map(|(name, table_type)| self.metadata_row(&name, &table_type))
            .collect()
    }

    fn metadata_row(&mut self, name: &str, table_type: &str) -> Result<Value> {
        let mut object = Map::new();
        object.insert("name".to_string(), Value::String(name.to_string()));
        object.insert("type".to_string(), Value::String(table_type.to_string()));
        object.insert(
            "columns".to_string(),
            Value::Array(self.table_columns(name)?),
        );
        object.insert("row_count".to_string(), json!(self.table_row_count(name)?));

        if name == "trace_range" {
            if let Some((start_ts, end_ts)) = self.trace_range_bounds(name)? {
                object.insert("start_ts".to_string(), nullable_i64(start_ts));
                object.insert("end_ts".to_string(), nullable_i64(end_ts));
            }
        }

        Ok(Value::Object(object))
    }

    fn table_columns(&mut self, name: &str) -> Result<Vec<Value>> {
        let sql = format!("PRAGMA table_info({})", quote_identifier(name)?);
        let mut stmt = self.conn.prepare(&sql)?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(Value::String)
            .collect();

        Ok(columns)
    }

    fn table_row_count(&mut self, name: &str) -> Result<i64> {
        let sql = format!("SELECT COUNT(*) FROM {}", quote_identifier(name)?);
        let row_count = self.conn.query_row(&sql, [], |row| row.get(0))?;

        Ok(row_count)
    }

    fn trace_range_bounds(&mut self, name: &str) -> Result<Option<(Option<i64>, Option<i64>)>> {
        let sql = format!(
            "SELECT start_ts, end_ts FROM {} LIMIT 1",
            quote_identifier(name)?
        );
        let bounds = self
            .conn
            .query_row(&sql, [], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;

        Ok(bounds)
    }
}

impl QueryClient for SqliteQueryClient {
    fn create_view(&mut self, name: &str, sql: &str) -> Result<()> {
        let view_name = quote_identifier(name)?;
        self.conn
            .execute(&format!("CREATE TEMP VIEW {view_name} AS {sql}"), [])?;

        Ok(())
    }

    fn query_window(&mut self, request: QueryWindowRequest) -> Result<Vec<Value>> {
        match request.mode {
            QueryWindowMode::Metadata => {
                if !request.filters.is_empty() {
                    bail!("metadata filters are unsupported");
                }

                self.metadata_rows(request.limit)
            }
            QueryWindowMode::Full | QueryWindowMode::Window => {
                let mut sql = format!("SELECT * FROM {}", quote_identifier(&request.target)?);
                let mut conditions = request
                    .filters
                    .iter()
                    .map(|(column, value)| filter_condition(column, value))
                    .collect::<Result<Vec<_>>>()?;

                if request.mode == QueryWindowMode::Window {
                    let time_column = request
                        .time_column
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("window query requires time_column"))?;
                    let start_ts = request
                        .start_ts
                        .ok_or_else(|| anyhow::anyhow!("window query requires start_ts"))?;
                    let end_ts = request
                        .end_ts
                        .ok_or_else(|| anyhow::anyhow!("window query requires end_ts"))?;
                    let time_expr = quote_identifier(time_column)?;
                    let duration_expr = match request.duration_column.as_deref() {
                        Some(duration_column) => {
                            format!("COALESCE({}, 0)", quote_identifier(duration_column)?)
                        }
                        None => "0".to_string(),
                    };
                    conditions.push(format!(
                        "{time_expr} + {duration_expr} > {start_ts} AND {time_expr} < {end_ts}"
                    ));
                }

                if !conditions.is_empty() {
                    sql.push_str(" WHERE ");
                    sql.push_str(&conditions.join(" AND "));
                }
                if let Some(limit) = request.limit {
                    sql.push_str(&format!(" LIMIT {limit}"));
                }

                self.query_rows(&sql)
            }
        }
    }
}

fn filter_condition(column: &str, value: &Value) -> Result<String> {
    let column = quote_identifier(column)?;
    match value {
        Value::Null => Ok(format!("{column} IS NULL")),
        Value::Bool(value) => Ok(format!("{column} = {}", i32::from(*value))),
        Value::Number(value) => Ok(format!("{column} = {value}")),
        Value::String(value) => Ok(format!("{column} = '{}'", escape_string_literal(value))),
        Value::Array(_) | Value::Object(_) => bail!("unsupported filter value for {column}"),
    }
}

fn quote_identifier(identifier: &str) -> Result<String> {
    if identifier.is_empty()
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.')
    {
        bail!("unsafe sqlite identifier: {identifier}");
    }

    identifier
        .split('.')
        .map(|part| {
            if part.is_empty() {
                bail!("unsafe sqlite identifier: {identifier}");
            }
            Ok(format!("\"{part}\""))
        })
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join("."))
}

fn escape_string_literal(value: &str) -> String {
    value.replace('\'', "''")
}

fn nullable_i64(value: Option<i64>) -> Value {
    value.map(Value::from).unwrap_or(Value::Null)
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
