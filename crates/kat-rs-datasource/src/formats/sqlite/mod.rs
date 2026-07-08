use std::{path::Path, str, sync::Arc};

use anyhow::{Context, Result, bail};
use arrow_array::{
    ArrayRef, RecordBatch,
    builder::{Float64Builder, Int64Builder, LargeBinaryBuilder, LargeStringBuilder},
};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use rusqlite::{
    Connection, Row,
    types::{Type, ValueRef},
};

use crate::dataset::DatasetTableWriter;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SqliteObject {
    pub(crate) name: String,
    pub(crate) kind: SqliteObjectKind,
    pub(crate) columns: Vec<SqliteColumn>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SqliteObjectKind {
    Table,
    View,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SqliteColumn {
    pub(crate) name: String,
    pub(crate) declared_type: String,
}

pub(crate) fn open(path: &Path) -> Result<Connection> {
    Connection::open(path)
        .with_context(|| format!("failed to open SQLite database: {}", path.display()))
}

pub(crate) fn objects(conn: &Connection) -> Result<Vec<SqliteObject>> {
    let mut stmt = conn.prepare(
        "select name, type from sqlite_master \
         where type in ('table', 'view') and name not like 'sqlite_%' \
         order by name",
    )?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(0)?;
        let kind_text: String = row.get(1)?;
        let kind = match kind_text.as_str() {
            "table" => SqliteObjectKind::Table,
            "view" => SqliteObjectKind::View,
            _ => unreachable!("sqlite_master query filters object kind"),
        };
        Ok((name, kind))
    })?;

    let mut objects = Vec::new();
    for row in rows {
        let (name, kind) = row?;
        let columns = columns(conn, &name)?;
        objects.push(SqliteObject {
            name,
            kind,
            columns,
        });
    }

    Ok(objects)
}

pub(crate) fn schema(object: &SqliteObject) -> Result<SchemaRef> {
    if object.columns.is_empty() {
        bail!("SQLite object {} has no columns", object.name);
    }

    let fields = object
        .columns
        .iter()
        .map(|column| Field::new(&column.name, arrow_type(&column.declared_type), true))
        .collect::<Vec<_>>();

    Ok(Arc::new(Schema::new(fields)))
}

pub(crate) fn stream_object(
    conn: &Connection,
    object: &SqliteObject,
    writer: &mut DatasetTableWriter,
    batch_size: usize,
) -> Result<()> {
    let query = select_all_sql(object)?;
    let mut stmt = conn
        .prepare(&query)
        .with_context(|| format!("failed to prepare SQLite object query: {query}"))?;
    let mut rows = stmt
        .query([])
        .with_context(|| format!("failed to query SQLite object {}", object.name))?;
    let mut builders = SqliteBatchBuilders::new(object)?;

    while let Some(row) = rows
        .next()
        .with_context(|| format!("failed to read SQLite object {}", object.name))?
    {
        builders.append_row(row)?;
        if builders.len() >= batch_size {
            writer.write(&builders.finish()?)?;
            builders = SqliteBatchBuilders::new(object)?;
        }
    }

    if builders.len() > 0 {
        writer.write(&builders.finish()?)?;
    }

    Ok(())
}

fn columns(conn: &Connection, object_name: &str) -> Result<Vec<SqliteColumn>> {
    let query = format!("pragma table_info({})", quote_sql_string(object_name));
    let mut stmt = conn
        .prepare(&query)
        .with_context(|| format!("failed to inspect SQLite object {object_name}"))?;
    let rows = stmt.query_map([], |row| {
        Ok(SqliteColumn {
            name: row.get(1)?,
            declared_type: row.get(2)?,
        })
    })?;

    let mut columns = Vec::new();
    for row in rows {
        columns.push(row?);
    }

    Ok(columns)
}

fn select_all_sql(object: &SqliteObject) -> Result<String> {
    if object.columns.is_empty() {
        bail!("SQLite object {} has no columns", object.name);
    }

    let columns = object
        .columns
        .iter()
        .map(|column| quote_identifier(&column.name))
        .collect::<Vec<_>>()
        .join(", ");

    Ok(format!(
        "select {columns} from {}",
        quote_identifier(&object.name)
    ))
}

fn arrow_type(declared: &str) -> DataType {
    let upper = declared.trim().to_ascii_uppercase();
    if upper.contains("INT") {
        DataType::Int64
    } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
        DataType::Float64
    } else if upper.contains("BLOB") {
        DataType::LargeBinary
    } else {
        DataType::LargeUtf8
    }
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn quote_sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

struct SqliteBatchBuilders {
    object_name: String,
    columns: Vec<SqliteColumnBuilder>,
    len: usize,
}

impl SqliteBatchBuilders {
    fn new(object: &SqliteObject) -> Result<Self> {
        let columns = object
            .columns
            .iter()
            .map(SqliteColumnBuilder::new)
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            object_name: object.name.clone(),
            columns,
            len: 0,
        })
    }

    fn append_row(&mut self, row: &Row<'_>) -> Result<()> {
        for (index, column) in self.columns.iter_mut().enumerate() {
            let value = row.get_ref(index).with_context(|| {
                format!(
                    "failed to read SQLite value {}.{}",
                    self.object_name, column.name
                )
            })?;
            column.append_value(&self.object_name, value)?;
        }
        self.len += 1;
        Ok(())
    }

    fn len(&self) -> usize {
        self.len
    }

    fn finish(self) -> Result<RecordBatch> {
        let fields = self
            .columns
            .iter()
            .map(|column| Field::new(&column.name, column.builder.data_type(), true))
            .collect::<Vec<_>>();
        let arrays = self
            .columns
            .into_iter()
            .map(SqliteColumnBuilder::finish)
            .collect::<Vec<_>>();

        RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
            .with_context(|| format!("failed to build SQLite batch for {}", self.object_name))
    }
}

struct SqliteColumnBuilder {
    name: String,
    builder: SqliteValueBuilder,
}

impl SqliteColumnBuilder {
    fn new(column: &SqliteColumn) -> Result<Self> {
        Ok(Self {
            name: column.name.clone(),
            builder: SqliteValueBuilder::new(&column.declared_type),
        })
    }

    fn append_value(&mut self, object_name: &str, value: ValueRef<'_>) -> Result<()> {
        self.builder
            .append_value(value)
            .with_context(|| sqlite_value_context(object_name, &self.name, value.data_type()))
    }

    fn finish(self) -> ArrayRef {
        self.builder.finish()
    }
}

enum SqliteValueBuilder {
    Int64(Int64Builder),
    Float64(Float64Builder),
    LargeUtf8 {
        builder: LargeStringBuilder,
        coerce_scalars: bool,
    },
    LargeBinary(LargeBinaryBuilder),
}

impl SqliteValueBuilder {
    fn new(declared_type: &str) -> Self {
        match arrow_type(declared_type) {
            DataType::Int64 => Self::Int64(Int64Builder::new()),
            DataType::Float64 => Self::Float64(Float64Builder::new()),
            DataType::LargeBinary => Self::LargeBinary(LargeBinaryBuilder::new()),
            _ => Self::LargeUtf8 {
                builder: LargeStringBuilder::new(),
                coerce_scalars: declared_type.trim().is_empty(),
            },
        }
    }

    fn data_type(&self) -> DataType {
        match self {
            Self::Int64(_) => DataType::Int64,
            Self::Float64(_) => DataType::Float64,
            Self::LargeUtf8 { .. } => DataType::LargeUtf8,
            Self::LargeBinary(_) => DataType::LargeBinary,
        }
    }

    fn append_value(&mut self, value: ValueRef<'_>) -> Result<()> {
        match (self, value) {
            (Self::Int64(builder), ValueRef::Null) => builder.append_null(),
            (Self::Int64(builder), ValueRef::Integer(value)) => builder.append_value(value),
            (Self::Float64(builder), ValueRef::Null) => builder.append_null(),
            (Self::Float64(builder), ValueRef::Integer(value)) => {
                builder.append_value(value as f64)
            }
            (Self::Float64(builder), ValueRef::Real(value)) => builder.append_value(value),
            (Self::LargeUtf8 { builder, .. }, ValueRef::Null) => builder.append_null(),
            (Self::LargeUtf8 { builder, .. }, ValueRef::Text(value)) => {
                builder.append_value(str::from_utf8(value).context("SQLite text is not UTF-8")?)
            }
            (
                Self::LargeUtf8 {
                    builder,
                    coerce_scalars: true,
                },
                ValueRef::Integer(value),
            ) => builder.append_value(value.to_string()),
            (
                Self::LargeUtf8 {
                    builder,
                    coerce_scalars: true,
                },
                ValueRef::Real(value),
            ) => builder.append_value(value.to_string()),
            (Self::LargeBinary(builder), ValueRef::Null) => builder.append_null(),
            (Self::LargeBinary(builder), ValueRef::Blob(value)) => builder.append_value(value),
            _ => bail!(
                "only values compatible with the declared SQLite column type can be converted"
            ),
        }

        Ok(())
    }

    fn finish(self) -> ArrayRef {
        match self {
            Self::Int64(mut builder) => Arc::new(builder.finish()),
            Self::Float64(mut builder) => Arc::new(builder.finish()),
            Self::LargeUtf8 { mut builder, .. } => Arc::new(builder.finish()),
            Self::LargeBinary(mut builder) => Arc::new(builder.finish()),
        }
    }
}

fn sqlite_value_context(object_name: &str, column_name: &str, value_type: Type) -> String {
    format!("failed to convert SQLite value {object_name}.{column_name} of type {value_type:?}")
}
