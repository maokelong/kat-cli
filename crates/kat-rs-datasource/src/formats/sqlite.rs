use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use arrow_array::{
    ArrayRef, Float64Array, Int64Array, RecordBatch, StringArray, builder::BinaryBuilder,
};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use rusqlite::{Connection, types::ValueRef};

const PACK_DEMO_TABLES: [&str; 5] = ["process", "thread", "callstack", "thread_state", "instant"];
const SQLITE_BATCH_ROWS: usize = 16 * 1024;

pub(crate) struct SqliteTable {
    pub(crate) name: &'static str,
    pub(crate) parquet_file_name: String,
    pub(crate) schema: SchemaRef,
    pub(crate) batches: Vec<RecordBatch>,
}

#[derive(Clone, Copy)]
enum SqliteColumnType {
    Int64,
    Float64,
    Utf8,
    Binary,
}

struct SqliteColumn {
    name: String,
    data_type: SqliteColumnType,
}

enum ColumnValues {
    Int64(Vec<Option<i64>>),
    Float64(Vec<Option<f64>>),
    Utf8(Vec<Option<String>>),
    Binary(Vec<Option<Vec<u8>>>),
}

pub(crate) fn read_pack_demo_tables(path: &Path) -> Result<Vec<SqliteTable>> {
    let connection = Connection::open(path)
        .with_context(|| format!("failed to open SQLite database: {}", path.display()))?;

    for table in PACK_DEMO_TABLES {
        ensure_table_exists(&connection, table)?;
    }

    PACK_DEMO_TABLES
        .into_iter()
        .map(|table| read_table(&connection, table))
        .collect()
}

fn ensure_table_exists(connection: &Connection, table: &str) -> Result<()> {
    let count: i64 = connection
        .query_row(
            "select count(*) from sqlite_master where type = 'table' and name = ?1",
            [table],
            |row| row.get(0),
        )
        .with_context(|| format!("failed to inspect SQLite table {table}"))?;
    if count == 0 {
        bail!("missing required SQLite table {table}");
    }

    Ok(())
}

fn read_table(connection: &Connection, table: &'static str) -> Result<SqliteTable> {
    let columns = table_columns(connection, table)?;
    let schema = schema_for(table, &columns);
    let sql = table_select_sql(table);
    let mut statement = connection
        .prepare(&sql)
        .with_context(|| format!("failed to prepare SQLite table read for {table}"))?;
    let mut rows = statement
        .query([])
        .with_context(|| format!("failed to query SQLite table {table}"))?;

    let mut batches = Vec::new();
    let mut values = empty_columns(&columns);
    let mut rows_in_batch = 0usize;

    while let Some(row) = rows
        .next()
        .with_context(|| format!("failed to read SQLite row from {table}"))?
    {
        for index in 0..columns.len() {
            let value = row
                .get_ref(index)
                .with_context(|| format!("failed to read SQLite value {table}.{index}"))?;
            push_value(table, &columns[index], &mut values[index], value)?;
        }
        rows_in_batch += 1;

        if rows_in_batch >= SQLITE_BATCH_ROWS {
            batches.push(values_to_batch(Arc::clone(&schema), &mut values)?);
            rows_in_batch = 0;
        }
    }

    if rows_in_batch > 0 || batches.is_empty() {
        batches.push(values_to_batch(Arc::clone(&schema), &mut values)?);
    }

    Ok(SqliteTable {
        name: table,
        parquet_file_name: format!("sqlite.{table}.parquet"),
        schema,
        batches,
    })
}

fn table_select_sql(table: &str) -> String {
    match table {
        "instant" => "select rowid as rowid, * from instant".to_string(),
        _ => format!("select * from {table}"),
    }
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<SqliteColumn>> {
    let mut columns = Vec::new();
    if table == "instant" {
        columns.push(SqliteColumn {
            name: "rowid".to_string(),
            data_type: SqliteColumnType::Int64,
        });
    }

    let pragma = format!("pragma table_info({table})");
    let mut statement = connection
        .prepare(&pragma)
        .with_context(|| format!("failed to inspect SQLite table columns for {table}"))?;
    let mut rows = statement
        .query([])
        .with_context(|| format!("failed to read SQLite table_info for {table}"))?;

    while let Some(row) = rows
        .next()
        .with_context(|| format!("failed to read SQLite table_info row for {table}"))?
    {
        let name: String = row.get(1)?;
        let declared_type: String = row.get(2)?;
        columns.push(SqliteColumn {
            name,
            data_type: sqlite_declared_type(&declared_type),
        });
    }

    Ok(columns)
}

fn sqlite_declared_type(value: &str) -> SqliteColumnType {
    let upper = value.to_ascii_uppercase();
    if upper.contains("INT") {
        SqliteColumnType::Int64
    } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
        SqliteColumnType::Float64
    } else if upper.contains("BLOB") {
        SqliteColumnType::Binary
    } else {
        SqliteColumnType::Utf8
    }
}

fn schema_for(table: &str, columns: &[SqliteColumn]) -> SchemaRef {
    let fields = columns
        .iter()
        .map(|column| {
            let data_type = match column.data_type {
                SqliteColumnType::Int64 => DataType::Int64,
                SqliteColumnType::Float64 => DataType::Float64,
                SqliteColumnType::Utf8 => DataType::Utf8,
                SqliteColumnType::Binary => DataType::Binary,
            };
            Field::new(&column.name, data_type, true)
        })
        .collect::<Vec<_>>();
    Arc::new(Schema::new_with_metadata(
        fields,
        [("sqlite_source_table".to_string(), table.to_string())].into(),
    ))
}

fn empty_columns(columns: &[SqliteColumn]) -> Vec<ColumnValues> {
    columns
        .iter()
        .map(|column| match column.data_type {
            SqliteColumnType::Int64 => ColumnValues::Int64(Vec::new()),
            SqliteColumnType::Float64 => ColumnValues::Float64(Vec::new()),
            SqliteColumnType::Utf8 => ColumnValues::Utf8(Vec::new()),
            SqliteColumnType::Binary => ColumnValues::Binary(Vec::new()),
        })
        .collect()
}

fn push_value(
    table: &str,
    column: &SqliteColumn,
    values: &mut ColumnValues,
    value: ValueRef<'_>,
) -> Result<()> {
    match (values, value) {
        (ColumnValues::Int64(values), ValueRef::Null) => values.push(None),
        (ColumnValues::Int64(values), ValueRef::Integer(value)) => values.push(Some(value)),
        (ColumnValues::Int64(values), ValueRef::Text(value)) => {
            values.push(Some(parse_sqlite_text::<i64>(table, column, value)?))
        }
        (ColumnValues::Float64(values), ValueRef::Null) => values.push(None),
        (ColumnValues::Float64(values), ValueRef::Integer(value)) => values.push(Some(value as f64)),
        (ColumnValues::Float64(values), ValueRef::Real(value)) => values.push(Some(value)),
        (ColumnValues::Float64(values), ValueRef::Text(value)) => {
            values.push(Some(parse_sqlite_text::<f64>(table, column, value)?))
        }
        (ColumnValues::Utf8(values), ValueRef::Null) => values.push(None),
        (ColumnValues::Utf8(values), ValueRef::Text(value)) => {
            values.push(Some(String::from_utf8_lossy(value).into_owned()))
        }
        (ColumnValues::Utf8(values), ValueRef::Integer(value)) => values.push(Some(value.to_string())),
        (ColumnValues::Utf8(values), ValueRef::Real(value)) => values.push(Some(value.to_string())),
        (ColumnValues::Binary(values), ValueRef::Null) => values.push(None),
        (ColumnValues::Binary(values), ValueRef::Blob(value)) => values.push(Some(value.to_vec())),
        (_, other) => bail!(
            "cannot convert SQLite value {other:?} for {table}.{}",
            column.name
        ),
    }

    Ok(())
}

fn parse_sqlite_text<T>(table: &str, column: &SqliteColumn, value: &[u8]) -> Result<T>
where
    T: std::str::FromStr,
{
    let text = std::str::from_utf8(value)
        .with_context(|| format!("cannot decode SQLite text for {table}.{}", column.name))?;
    text.parse::<T>().map_err(|_| {
        anyhow::anyhow!(
            "cannot convert SQLite text value {:?} for {}.{}",
            text,
            table,
            column.name
        )
    })
}

fn values_to_batch(schema: SchemaRef, values: &mut [ColumnValues]) -> Result<RecordBatch> {
    let columns = values
        .iter_mut()
        .map(|values| {
            let taken = match values {
                ColumnValues::Int64(_) => std::mem::replace(values, ColumnValues::Int64(Vec::new())),
                ColumnValues::Float64(_) => {
                    std::mem::replace(values, ColumnValues::Float64(Vec::new()))
                }
                ColumnValues::Utf8(_) => std::mem::replace(values, ColumnValues::Utf8(Vec::new())),
                ColumnValues::Binary(_) => {
                    std::mem::replace(values, ColumnValues::Binary(Vec::new()))
                }
            };

            match taken {
                ColumnValues::Int64(values) => Arc::new(Int64Array::from(values)) as ArrayRef,
                ColumnValues::Float64(values) => Arc::new(Float64Array::from(values)) as ArrayRef,
                ColumnValues::Utf8(values) => Arc::new(StringArray::from(values)) as ArrayRef,
                ColumnValues::Binary(values) => {
                    let mut builder = BinaryBuilder::new();
                    for value in values {
                        match value {
                            Some(value) => builder.append_value(value),
                            None => builder.append_null(),
                        }
                    }
                    Arc::new(builder.finish()) as ArrayRef
                }
            }
        })
        .collect::<Vec<_>>();

    RecordBatch::try_new(schema, columns).context("failed to build SQLite Arrow record batch")
}
