use std::{path::Path, sync::Arc};

use anyhow::{Context, Result};
use arrow_array::{
    ArrayRef, RecordBatch,
    builder::{Int64Builder, StringBuilder},
};
use arrow_schema::{DataType, Field, Schema};
use rusqlite::{Connection, Row};

pub(crate) struct SqliteTable {
    pub(crate) logical_name: &'static str,
    pub(crate) parquet_file_name: String,
    pub(crate) batch: RecordBatch,
}

pub(crate) fn openharmony_tables(path: &Path) -> Result<Vec<SqliteTable>> {
    let connection = Connection::open(path)
        .with_context(|| format!("failed to open SQLite source: {}", path.display()))?;

    OPENHARMONY_TABLES
        .iter()
        .map(|spec| read_table(&connection, spec))
        .collect()
}

const OPENHARMONY_TABLES: &[SqliteTableSpec] = &[
    SqliteTableSpec {
        name: "process",
        columns: &[
            SqliteColumnSpec::int64("id"),
            SqliteColumnSpec::int64("ipid"),
            SqliteColumnSpec::int64("pid"),
            SqliteColumnSpec::utf8("name"),
        ],
    },
    SqliteTableSpec {
        name: "thread",
        columns: &[
            SqliteColumnSpec::int64("id"),
            SqliteColumnSpec::int64("itid"),
            SqliteColumnSpec::int64("tid"),
            SqliteColumnSpec::utf8("name"),
            SqliteColumnSpec::int64("ipid"),
            SqliteColumnSpec::int64("is_main_thread"),
        ],
    },
    SqliteTableSpec {
        name: "callstack",
        columns: &[
            SqliteColumnSpec::int64("id"),
            SqliteColumnSpec::int64("ts"),
            SqliteColumnSpec::int64("dur"),
            SqliteColumnSpec::int64("callid"),
            SqliteColumnSpec::utf8("name"),
            SqliteColumnSpec::int64("depth"),
            SqliteColumnSpec::int64("parent_id"),
        ],
    },
    SqliteTableSpec {
        name: "thread_state",
        columns: &[
            SqliteColumnSpec::int64("id"),
            SqliteColumnSpec::int64("ts"),
            SqliteColumnSpec::int64("dur"),
            SqliteColumnSpec::int64("itid"),
            SqliteColumnSpec::int64("tid"),
            SqliteColumnSpec::utf8("state"),
        ],
    },
    SqliteTableSpec {
        name: "instant",
        columns: &[
            SqliteColumnSpec::int64_expr("rowid", "rowid"),
            SqliteColumnSpec::int64("ts"),
            SqliteColumnSpec::utf8("name"),
            SqliteColumnSpec::int64("ref"),
            SqliteColumnSpec::int64("wakeup_from"),
            SqliteColumnSpec::utf8("ref_type"),
        ],
    },
];

struct SqliteTableSpec {
    name: &'static str,
    columns: &'static [SqliteColumnSpec],
}

#[derive(Clone, Copy)]
struct SqliteColumnSpec {
    name: &'static str,
    select_expression: &'static str,
    data_type: SqliteColumnType,
}

impl SqliteColumnSpec {
    const fn int64(name: &'static str) -> Self {
        Self::int64_expr(name, name)
    }

    const fn int64_expr(name: &'static str, select_expression: &'static str) -> Self {
        Self {
            name,
            select_expression,
            data_type: SqliteColumnType::Int64,
        }
    }

    const fn utf8(name: &'static str) -> Self {
        Self {
            name,
            select_expression: name,
            data_type: SqliteColumnType::Utf8,
        }
    }
}

#[derive(Clone, Copy)]
enum SqliteColumnType {
    Int64,
    Utf8,
}

impl SqliteColumnType {
    fn arrow_data_type(self) -> DataType {
        match self {
            Self::Int64 => DataType::Int64,
            Self::Utf8 => DataType::Utf8,
        }
    }
}

fn read_table(connection: &Connection, spec: &SqliteTableSpec) -> Result<SqliteTable> {
    let select_list = spec
        .columns
        .iter()
        .map(|column| column.select_expression)
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("select {select_list} from {}", spec.name);
    let mut statement = connection
        .prepare(&sql)
        .with_context(|| format!("failed to prepare SQLite table extraction: {sql}"))?;
    let mut rows = statement
        .query([])
        .with_context(|| format!("failed to query SQLite table {}", spec.name))?;
    let mut builders = spec
        .columns
        .iter()
        .map(|column| ColumnBuilder::new(column.data_type))
        .collect::<Vec<_>>();

    while let Some(row) = rows
        .next()
        .with_context(|| format!("failed to read SQLite table {}", spec.name))?
    {
        for (index, builder) in builders.iter_mut().enumerate() {
            builder.append(row, index, spec.name, spec.columns[index].name)?;
        }
    }

    let fields = spec
        .columns
        .iter()
        .map(|column| Field::new(column.name, column.data_type.arrow_data_type(), true))
        .collect::<Vec<_>>();
    let columns = builders
        .into_iter()
        .map(ColumnBuilder::finish)
        .collect::<Vec<_>>();
    let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns)
        .with_context(|| format!("failed to build Arrow batch for SQLite table {}", spec.name))?;

    Ok(SqliteTable {
        logical_name: spec.name,
        parquet_file_name: format!("sqlite.{}.parquet", spec.name),
        batch,
    })
}

enum ColumnBuilder {
    Int64(Int64Builder),
    Utf8(StringBuilder),
}

impl ColumnBuilder {
    fn new(data_type: SqliteColumnType) -> Self {
        match data_type {
            SqliteColumnType::Int64 => Self::Int64(Int64Builder::new()),
            SqliteColumnType::Utf8 => Self::Utf8(StringBuilder::new()),
        }
    }

    fn append(&mut self, row: &Row<'_>, index: usize, table: &str, column: &str) -> Result<()> {
        match self {
            Self::Int64(builder) => {
                let value = row
                    .get::<_, Option<i64>>(index)
                    .with_context(|| format!("failed to read SQLite column {table}.{column}"))?;
                builder.append_option(value);
            }
            Self::Utf8(builder) => {
                let value = row
                    .get::<_, Option<String>>(index)
                    .with_context(|| format!("failed to read SQLite column {table}.{column}"))?;
                match value {
                    Some(value) => builder.append_value(value),
                    None => builder.append_null(),
                }
            }
        }

        Ok(())
    }

    fn finish(mut self) -> ArrayRef {
        match &mut self {
            Self::Int64(builder) => Arc::new(builder.finish()),
            Self::Utf8(builder) => Arc::new(builder.finish()),
        }
    }
}
