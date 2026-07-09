use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use arrow_array::{
    ArrayRef, BinaryArray, Float64Array, Int64Array, RecordBatch, StringArray,
    builder::{BinaryBuilder, Float64Builder, Int64Builder, StringBuilder},
};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use rusqlite::{Connection, Row, types::ValueRef};

const SQLITE_BATCH_ROWS: usize = 8192;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SqliteTable {
    pub(crate) name: String,
    pub(crate) columns: Vec<SqliteColumn>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SqliteColumn {
    pub(crate) name: String,
    pub(crate) arrow_type: SqliteArrowType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SqliteArrowType {
    Int64,
    Float64,
    Utf8,
    Binary,
}

pub(crate) fn discover_tables(connection: &Connection) -> Result<Vec<SqliteTable>> {
    let mut statement = connection.prepare(
        "select name from sqlite_master \
         where type = 'table' and name not like 'sqlite_%' \
         order by name",
    )?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    names
        .into_iter()
        .map(|name| discover_table(connection, name))
        .collect()
}

pub(crate) fn stream_table_batches(
    connection: &Connection,
    table: &SqliteTable,
    mut on_batch: impl FnMut(RecordBatch) -> Result<()>,
) -> Result<()> {
    let schema = table_schema(table);
    let sql = format!("select * from {}", quote_identifier(&table.name));
    let mut statement = connection
        .prepare(&sql)
        .with_context(|| format!("failed to prepare SQLite table read for {}", table.name))?;
    let mut rows = statement
        .query([])
        .with_context(|| format!("failed to read SQLite table {}", table.name))?;
    let mut builders = BatchBuilders::new(table);
    let mut row_count = 0_usize;
    let mut wrote_any_batch = false;

    while let Some(row) = rows.next()? {
        builders.append_row(table, row)?;
        row_count += 1;

        if row_count == SQLITE_BATCH_ROWS {
            on_batch(builders.finish(schema.clone())?)?;
            builders = BatchBuilders::new(table);
            row_count = 0;
            wrote_any_batch = true;
        }
    }

    if row_count > 0 || !wrote_any_batch {
        on_batch(builders.finish(schema)?)?;
    }

    Ok(())
}

pub(crate) fn open(path: &Path) -> Result<Connection> {
    Connection::open(path)
        .with_context(|| format!("failed to open SQLite database: {}", path.display()))
}

fn discover_table(connection: &Connection, name: String) -> Result<SqliteTable> {
    let pragma = format!("pragma table_info({})", quote_string_literal(&name));
    let mut statement = connection.prepare(&pragma)?;
    let columns = statement
        .query_map([], |row| {
            let column_name = row.get::<_, String>(1)?;
            let declared_type = row.get::<_, String>(2)?;
            Ok(SqliteColumn {
                name: column_name,
                arrow_type: sqlite_arrow_type(&declared_type),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    if columns.is_empty() {
        bail!("SQLite table {name} has no columns");
    }

    Ok(SqliteTable { name, columns })
}

fn sqlite_arrow_type(declared_type: &str) -> SqliteArrowType {
    let upper = declared_type.to_ascii_uppercase();
    if upper.contains("BLOB") {
        SqliteArrowType::Binary
    } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
        SqliteArrowType::Float64
    } else if upper.contains("INT") {
        SqliteArrowType::Int64
    } else {
        SqliteArrowType::Utf8
    }
}

fn table_schema(table: &SqliteTable) -> SchemaRef {
    let fields = table
        .columns
        .iter()
        .map(|column| {
            let data_type = match column.arrow_type {
                SqliteArrowType::Int64 => DataType::Int64,
                SqliteArrowType::Float64 => DataType::Float64,
                SqliteArrowType::Utf8 => DataType::Utf8,
                SqliteArrowType::Binary => DataType::Binary,
            };
            Field::new(&column.name, data_type, true)
        })
        .collect::<Vec<_>>();
    Arc::new(Schema::new(fields))
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn quote_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

enum ColumnBuilder {
    Int64(Int64Builder),
    Float64(Float64Builder),
    Utf8(StringBuilder),
    Binary(BinaryBuilder),
}

struct BatchBuilders {
    columns: Vec<ColumnBuilder>,
}

impl BatchBuilders {
    fn new(table: &SqliteTable) -> Self {
        let columns = table
            .columns
            .iter()
            .map(|column| match column.arrow_type {
                SqliteArrowType::Int64 => ColumnBuilder::Int64(Int64Builder::new()),
                SqliteArrowType::Float64 => ColumnBuilder::Float64(Float64Builder::new()),
                SqliteArrowType::Utf8 => ColumnBuilder::Utf8(StringBuilder::new()),
                SqliteArrowType::Binary => ColumnBuilder::Binary(BinaryBuilder::new()),
            })
            .collect();
        Self { columns }
    }

    fn append_row(&mut self, table: &SqliteTable, row: &Row<'_>) -> Result<()> {
        for (index, column) in table.columns.iter().enumerate() {
            let value = row.get_ref(index).with_context(|| {
                format!("failed to read SQLite value {}.{}", table.name, column.name)
            })?;
            self.columns[index].append_value(value).with_context(|| {
                format!(
                    "failed to convert SQLite value {}.{}",
                    table.name, column.name
                )
            })?;
        }
        Ok(())
    }

    fn finish(self, schema: SchemaRef) -> Result<RecordBatch> {
        let arrays = self
            .columns
            .into_iter()
            .map(ColumnBuilder::finish)
            .collect::<Vec<_>>();
        RecordBatch::try_new(schema, arrays).context("failed to build SQLite RecordBatch")
    }
}

impl ColumnBuilder {
    fn append_value(&mut self, value: ValueRef<'_>) -> Result<()> {
        match self {
            ColumnBuilder::Int64(builder) => match value {
                ValueRef::Null => builder.append_null(),
                ValueRef::Integer(value) => builder.append_value(value),
                ValueRef::Real(value) => builder.append_value(value as i64),
                other => bail!("expected integer-compatible SQLite value, got {other:?}"),
            },
            ColumnBuilder::Float64(builder) => match value {
                ValueRef::Null => builder.append_null(),
                ValueRef::Integer(value) => builder.append_value(value as f64),
                ValueRef::Real(value) => builder.append_value(value),
                other => bail!("expected real-compatible SQLite value, got {other:?}"),
            },
            ColumnBuilder::Utf8(builder) => match value {
                ValueRef::Null => builder.append_null(),
                ValueRef::Integer(value) => builder.append_value(value.to_string()),
                ValueRef::Real(value) => builder.append_value(value.to_string()),
                ValueRef::Text(value) => builder.append_value(std::str::from_utf8(value)?),
                ValueRef::Blob(value) => builder.append_value(hex_lower(value)),
            },
            ColumnBuilder::Binary(builder) => match value {
                ValueRef::Null => builder.append_null(),
                ValueRef::Blob(value) => builder.append_value(value),
                ValueRef::Text(value) => builder.append_value(value),
                other => bail!("expected blob-compatible SQLite value, got {other:?}"),
            },
        }
        Ok(())
    }

    fn finish(self) -> ArrayRef {
        match self {
            ColumnBuilder::Int64(mut builder) => Arc::new(builder.finish()) as Arc<Int64Array>,
            ColumnBuilder::Float64(mut builder) => Arc::new(builder.finish()) as Arc<Float64Array>,
            ColumnBuilder::Utf8(mut builder) => Arc::new(builder.finish()) as Arc<StringArray>,
            ColumnBuilder::Binary(mut builder) => Arc::new(builder.finish()) as Arc<BinaryArray>,
        }
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
