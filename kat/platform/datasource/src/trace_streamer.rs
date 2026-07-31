use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow_array::{
    ArrayRef, RecordBatch,
    builder::{Float64Builder, Int64Builder, StringBuilder},
};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use rusqlite::{Connection, OpenFlags, Row, types::ValueRef};

use crate::dataset_writer::{DatasetWriteError, DatasetWriter};
use crate::{DatasetWriteTarget, valid_table_name};

const BATCH_ROWS: usize = 8_192;

#[derive(Debug)]
pub struct ImportedDataset {
    path: PathBuf,
}

impl ImportedDataset {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// 通过 Deprecated Trace Streamer SQLite 表界面验证预发布 Data Import 机制。
///
/// 该入口不提供长期兼容承诺，并将在第一次正式发布前删除。目标覆盖开始前只检查
/// relation 定义与读取语句的列形状；数据行读取和 cell 转换发生在目标被破坏式清空之后，
/// 因而失败不会恢复旧内容。每个 relation 通过独立、无排序的读取语句物化，不保证来源
/// 行序稳定，也不保证多个 relation 来自同一个读取快照。
pub fn import_deprecated_trace_streamer(
    database: &Path,
    target: DatasetWriteTarget,
) -> Result<ImportedDataset, TraceStreamerImportError> {
    let metadata =
        fs::metadata(database).map_err(|source| TraceStreamerImportError::InspectDatabase {
            path: database.to_path_buf(),
            source,
        })?;
    if !metadata.is_file() {
        return Err(TraceStreamerImportError::DatabaseNotFile {
            path: database.to_path_buf(),
        });
    }
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|source| TraceStreamerImportError::OpenDatabase {
            path: database.to_path_buf(),
            source,
        })?;
    let relations = discover_relations(&connection)?;
    if relations.is_empty() {
        return Err(TraceStreamerImportError::NoRelations);
    }
    validate_relation_read_shapes(&connection, &relations)?;
    let mut writer =
        DatasetWriter::begin(target).map_err(TraceStreamerImportError::WriteDataset)?;
    for relation in relations {
        materialize_relation(&connection, &mut writer, relation)?;
    }
    let path = writer
        .finish()
        .map_err(TraceStreamerImportError::WriteDataset)?;
    Ok(ImportedDataset { path })
}

#[derive(Debug)]
struct Relation {
    name: String,
    columns: Vec<Column>,
}

#[derive(Clone, Copy, Debug)]
enum ColumnType {
    Integer,
    Real,
    Text,
}

#[derive(Debug)]
struct Column {
    name: String,
    data_type: ColumnType,
}

fn discover_relations(connection: &Connection) -> Result<Vec<Relation>, TraceStreamerImportError> {
    let mut statement = connection
        .prepare("PRAGMA table_list")
        .map_err(TraceStreamerImportError::EnumerateRelations)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(TraceStreamerImportError::EnumerateRelations)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(TraceStreamerImportError::EnumerateRelations)?;
    let mut names = rows
        .into_iter()
        .filter_map(|(schema, name, relation_type)| {
            (schema == "main" && relation_type == "table" && !name.starts_with("sqlite_"))
                .then_some(name)
        })
        .collect::<Vec<_>>();
    names.sort();
    names
        .into_iter()
        .map(|name| discover_relation(connection, name))
        .collect()
}

fn discover_relation(
    connection: &Connection,
    name: String,
) -> Result<Relation, TraceStreamerImportError> {
    if !valid_table_name(&name) {
        return Err(TraceStreamerImportError::InvalidRelationName { relation: name });
    }
    let sql = format!("PRAGMA table_xinfo({})", quote_string(&name));
    let mut statement =
        connection
            .prepare(&sql)
            .map_err(|source| TraceStreamerImportError::InspectRelation {
                relation: name.clone(),
                source,
            })?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })
        .map_err(|source| TraceStreamerImportError::InspectRelation {
            relation: name.clone(),
            source,
        })?;
    let mut columns = Vec::new();
    let mut names = HashSet::new();
    for row in rows {
        let (column, declaration) =
            row.map_err(|source| TraceStreamerImportError::InspectRelation {
                relation: name.clone(),
                source,
            })?;
        if !names.insert(column.clone()) {
            return Err(TraceStreamerImportError::DuplicateColumn {
                relation: name,
                column,
            });
        }
        let data_type = match declaration.trim().to_ascii_uppercase().as_str() {
            "INTEGER" | "INT" => ColumnType::Integer,
            "REAL" => ColumnType::Real,
            "TEXT" => ColumnType::Text,
            _ => {
                return Err(TraceStreamerImportError::UnsupportedDeclaredType {
                    relation: name,
                    column,
                    declaration,
                });
            }
        };
        columns.push(Column {
            name: column,
            data_type,
        });
    }
    if columns.is_empty() {
        return Err(TraceStreamerImportError::NoColumns { relation: name });
    }
    Ok(Relation { name, columns })
}

fn validate_relation_read_shapes(
    connection: &Connection,
    relations: &[Relation],
) -> Result<(), TraceStreamerImportError> {
    for relation in relations {
        let sql = format!("SELECT * FROM {}", quote_identifier(&relation.name));
        let statement = connection.prepare(&sql).map_err(|source| {
            TraceStreamerImportError::PrepareRelationRead {
                relation: relation.name.clone(),
                source,
            }
        })?;
        if statement.column_count() != relation.columns.len() {
            return Err(TraceStreamerImportError::ReadColumnCount {
                relation: relation.name.clone(),
                declared: relation.columns.len(),
                readable: statement.column_count(),
            });
        }
        for (index, column) in relation.columns.iter().enumerate() {
            let readable = statement.column_name(index).map_err(|source| {
                TraceStreamerImportError::ReadColumnMetadata {
                    relation: relation.name.clone(),
                    index,
                    source,
                }
            })?;
            if readable != column.name {
                return Err(TraceStreamerImportError::ReadColumnName {
                    relation: relation.name.clone(),
                    index,
                    declared: column.name.clone(),
                    readable: readable.to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn materialize_relation(
    connection: &Connection,
    writer: &mut DatasetWriter,
    relation: Relation,
) -> Result<(), TraceStreamerImportError> {
    let schema = relation_schema(&relation);
    let mut table = writer
        .begin_table(&relation.name, schema.clone())
        .map_err(TraceStreamerImportError::WriteDataset)?;
    let sql = format!("SELECT * FROM {}", quote_identifier(&relation.name));
    let mut statement =
        connection
            .prepare(&sql)
            .map_err(|source| TraceStreamerImportError::ReadRelation {
                relation: relation.name.clone(),
                source,
            })?;
    let mut rows =
        statement
            .query([])
            .map_err(|source| TraceStreamerImportError::ReadRelation {
                relation: relation.name.clone(),
                source,
            })?;
    let mut builders = Builders::new(&relation);
    let mut buffered = 0;
    let mut row_number = 0_u64;
    while let Some(row) = rows
        .next()
        .map_err(|source| TraceStreamerImportError::ReadRelation {
            relation: relation.name.clone(),
            source,
        })?
    {
        row_number += 1;
        builders.append(&relation, row, row_number)?;
        buffered += 1;
        if buffered == BATCH_ROWS {
            table
                .write(&builders.finish(schema.clone())?)
                .map_err(TraceStreamerImportError::WriteDataset)?;
            builders = Builders::new(&relation);
            buffered = 0;
        }
    }
    if buffered > 0 {
        table
            .write(&builders.finish(schema)?)
            .map_err(TraceStreamerImportError::WriteDataset)?;
    }
    table
        .finish()
        .map_err(TraceStreamerImportError::WriteDataset)
}

fn relation_schema(relation: &Relation) -> SchemaRef {
    Arc::new(Schema::new(
        relation
            .columns
            .iter()
            .map(|column| {
                let data_type = match column.data_type {
                    ColumnType::Integer => DataType::Int64,
                    ColumnType::Real => DataType::Float64,
                    ColumnType::Text => DataType::Utf8,
                };
                Field::new(&column.name, data_type, true)
            })
            .collect::<Vec<_>>(),
    ))
}

enum Builder {
    Integer(Int64Builder),
    Real(Float64Builder),
    Text(StringBuilder),
}

struct Builders(Vec<Builder>);

impl Builders {
    fn new(relation: &Relation) -> Self {
        Self(
            relation
                .columns
                .iter()
                .map(|column| match column.data_type {
                    ColumnType::Integer => Builder::Integer(Int64Builder::new()),
                    ColumnType::Real => Builder::Real(Float64Builder::new()),
                    ColumnType::Text => Builder::Text(StringBuilder::new()),
                })
                .collect(),
        )
    }

    fn append(
        &mut self,
        relation: &Relation,
        row: &Row<'_>,
        row_number: u64,
    ) -> Result<(), TraceStreamerImportError> {
        for (index, (column, builder)) in relation.columns.iter().zip(&mut self.0).enumerate() {
            let value =
                row.get_ref(index)
                    .map_err(|source| TraceStreamerImportError::ReadCell {
                        relation: relation.name.clone(),
                        column: column.name.clone(),
                        row: row_number,
                        source,
                    })?;
            match (builder, value) {
                (Builder::Integer(builder), ValueRef::Null) => {
                    builder.append_null();
                }
                (Builder::Integer(builder), ValueRef::Integer(value)) => {
                    builder.append_value(value);
                }
                (Builder::Real(builder), ValueRef::Null) => {
                    builder.append_null();
                }
                (Builder::Real(builder), ValueRef::Integer(value)) => {
                    if let Some(value) = exact_f64(value) {
                        builder.append_value(value);
                    } else {
                        return Err(TraceStreamerImportError::LossyRealCell {
                            relation: relation.name.clone(),
                            column: column.name.clone(),
                            row: row_number,
                            value,
                        });
                    }
                }
                (Builder::Real(builder), ValueRef::Real(value)) if value.is_finite() => {
                    builder.append_value(value);
                }
                (Builder::Real(_), ValueRef::Real(value)) => {
                    return Err(TraceStreamerImportError::NonFiniteRealCell {
                        relation: relation.name.clone(),
                        column: column.name.clone(),
                        row: row_number,
                        value,
                    });
                }
                (Builder::Text(builder), ValueRef::Null) => {
                    builder.append_null();
                }
                (Builder::Text(builder), ValueRef::Text(value)) => {
                    let value = std::str::from_utf8(value).map_err(|source| {
                        TraceStreamerImportError::InvalidUtf8TextCell {
                            relation: relation.name.clone(),
                            column: column.name.clone(),
                            row: row_number,
                            source,
                        }
                    })?;
                    builder.append_value(value);
                }
                _ => {
                    return Err(TraceStreamerImportError::ConvertCell {
                        relation: relation.name.clone(),
                        column: column.name.clone(),
                        row: row_number,
                        storage_class: sqlite_storage_class(value),
                    });
                }
            }
        }
        Ok(())
    }

    fn finish(self, schema: SchemaRef) -> Result<RecordBatch, TraceStreamerImportError> {
        let arrays = self
            .0
            .into_iter()
            .map(|builder| match builder {
                Builder::Integer(mut builder) => Arc::new(builder.finish()) as ArrayRef,
                Builder::Real(mut builder) => Arc::new(builder.finish()) as ArrayRef,
                Builder::Text(mut builder) => Arc::new(builder.finish()) as ArrayRef,
            })
            .collect();
        RecordBatch::try_new(schema, arrays).map_err(TraceStreamerImportError::BuildBatch)
    }
}

fn exact_f64(value: i64) -> Option<f64> {
    let magnitude = value.unsigned_abs();
    if magnitude == 0 {
        return Some(0.0);
    }
    let significant_bits = u64::BITS - magnitude.leading_zeros() - magnitude.trailing_zeros();
    (significant_bits <= f64::MANTISSA_DIGITS).then_some(value as f64)
}

fn sqlite_storage_class(value: ValueRef<'_>) -> &'static str {
    match value {
        ValueRef::Null => "NULL",
        ValueRef::Integer(_) => "INTEGER",
        ValueRef::Real(_) => "REAL",
        ValueRef::Text(_) => "TEXT",
        ValueRef::Blob(_) => "BLOB",
    }
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn quote_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[derive(Debug, thiserror::Error)]
pub enum TraceStreamerImportError {
    #[error("failed to inspect Trace Streamer database {path}")]
    InspectDatabase {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Trace Streamer database path is not a regular file: {path}")]
    DatabaseNotFile { path: PathBuf },
    #[error("failed to open Trace Streamer database {path} read-only")]
    OpenDatabase {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("failed to enumerate Trace Streamer SQLite relations")]
    EnumerateRelations(#[source] rusqlite::Error),
    #[error("Trace Streamer database has no queryable non-system relations")]
    NoRelations,
    #[error("SQLite relation name cannot be used as a Dataset table name: {relation:?}")]
    InvalidRelationName { relation: String },
    #[error("failed to inspect SQLite relation {relation:?}")]
    InspectRelation {
        relation: String,
        #[source]
        source: rusqlite::Error,
    },
    #[error("SQLite relation {relation:?} has no columns")]
    NoColumns { relation: String },
    #[error("SQLite relation {relation:?} has duplicate column {column:?}")]
    DuplicateColumn { relation: String, column: String },
    #[error(
        "unsupported SQLite declared type {declaration:?} for {relation}.{column}; expected INTEGER, REAL, or TEXT"
    )]
    UnsupportedDeclaredType {
        relation: String,
        column: String,
        declaration: String,
    },
    #[error("failed to prepare a read for SQLite relation {relation:?}")]
    PrepareRelationRead {
        relation: String,
        #[source]
        source: rusqlite::Error,
    },
    #[error(
        "SQLite relation {relation:?} exposes {readable} readable columns but declares {declared}"
    )]
    ReadColumnCount {
        relation: String,
        declared: usize,
        readable: usize,
    },
    #[error("failed to read column {index} metadata for SQLite relation {relation:?}")]
    ReadColumnMetadata {
        relation: String,
        index: usize,
        #[source]
        source: rusqlite::Error,
    },
    #[error(
        "SQLite relation {relation:?} column {index} is declared as {declared:?} but reads as {readable:?}"
    )]
    ReadColumnName {
        relation: String,
        index: usize,
        declared: String,
        readable: String,
    },
    #[error("failed to read SQLite relation {relation:?}")]
    ReadRelation {
        relation: String,
        #[source]
        source: rusqlite::Error,
    },
    #[error("failed to read SQLite cell {relation}.{column} at row {row}")]
    ReadCell {
        relation: String,
        column: String,
        row: u64,
        #[source]
        source: rusqlite::Error,
    },
    #[error(
        "cannot convert SQLite cell {relation}.{column} at row {row}: storage class {storage_class} does not match its declared type"
    )]
    ConvertCell {
        relation: String,
        column: String,
        row: u64,
        storage_class: &'static str,
    },
    #[error(
        "cannot convert SQLite cell {relation}.{column} at row {row}: INTEGER {value} cannot be represented exactly as Float64"
    )]
    LossyRealCell {
        relation: String,
        column: String,
        row: u64,
        value: i64,
    },
    #[error(
        "cannot convert SQLite cell {relation}.{column} at row {row}: REAL {value} is not finite"
    )]
    NonFiniteRealCell {
        relation: String,
        column: String,
        row: u64,
        value: f64,
    },
    #[error("cannot decode SQLite TEXT cell {relation}.{column} at row {row} as UTF-8")]
    InvalidUtf8TextCell {
        relation: String,
        column: String,
        row: u64,
        #[source]
        source: std::str::Utf8Error,
    },
    #[error("failed to build an Arrow batch from a Trace Streamer relation")]
    BuildBatch(#[source] arrow_schema::ArrowError),
    #[error("failed to write the Dataset")]
    WriteDataset(#[source] DatasetWriteError),
}
