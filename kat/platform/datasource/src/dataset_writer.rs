use std::{
    collections::HashSet,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow_array::RecordBatch;
use arrow_schema::Schema;
use parquet::{
    arrow::{
        ArrowWriter,
        arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions},
    },
    errors::ParquetError,
};

use crate::valid_table_name;

const MARKER: &str = ".kat-dataset";

#[derive(Clone, Debug)]
pub struct DatasetWriteTarget {
    path: PathBuf,
    overwrite: bool,
}

impl DatasetWriteTarget {
    pub fn new(path: impl Into<PathBuf>, overwrite: bool) -> Self {
        Self {
            path: path.into(),
            overwrite,
        }
    }
}

pub(crate) struct DatasetWriter {
    root: PathBuf,
    tables: PathBuf,
    table_names: HashSet<String>,
}

impl DatasetWriter {
    pub(crate) fn begin(target: DatasetWriteTarget) -> Result<Self, DatasetWriteError> {
        let root = prepare_target(&target)?;
        let tables = root.join("tables");
        fs::create_dir(&tables).map_err(|source| DatasetWriteError::CreateTables {
            path: tables.clone(),
            source,
        })?;
        Ok(Self {
            root,
            tables,
            table_names: HashSet::new(),
        })
    }

    pub(crate) fn begin_table(
        &mut self,
        name: &str,
        schema: Arc<Schema>,
    ) -> Result<DatasetTableWriter, DatasetWriteError> {
        if !valid_table_name(name) {
            return Err(DatasetWriteError::InvalidTableName {
                name: name.to_owned(),
            });
        }
        if !self.table_names.insert(name.to_owned()) {
            return Err(DatasetWriteError::DuplicateTable {
                name: name.to_owned(),
            });
        }
        let mut columns = HashSet::new();
        for field in schema.fields() {
            if !columns.insert(field.name().as_str()) {
                return Err(DatasetWriteError::DuplicateColumn {
                    table: name.to_owned(),
                    column: field.name().clone(),
                });
            }
        }
        let path = self.tables.join(format!("{name}.parquet"));
        let file = File::create(&path).map_err(|source| DatasetWriteError::CreateTable {
            table: name.to_owned(),
            path: path.clone(),
            source,
        })?;
        let writer = ArrowWriter::try_new(file, schema, None).map_err(|source| {
            DatasetWriteError::OpenTableWriter {
                table: name.to_owned(),
                path: path.clone(),
                source,
            }
        })?;
        Ok(DatasetTableWriter {
            table: name.to_owned(),
            path,
            writer,
        })
    }

    pub(crate) fn finish(self) -> Result<PathBuf, DatasetWriteError> {
        let mut tables = self
            .table_names
            .into_iter()
            .map(|name| (name.clone(), self.tables.join(format!("{name}.parquet"))))
            .collect::<Vec<_>>();
        tables.sort_by(|left, right| left.0.cmp(&right.0));
        for (table, path) in tables {
            let file =
                File::open(&path).map_err(|source| DatasetWriteError::ValidateTableOpen {
                    table: table.clone(),
                    path: path.clone(),
                    source,
                })?;
            ArrowReaderMetadata::load(&file, ArrowReaderOptions::default()).map_err(|source| {
                DatasetWriteError::ValidateTable {
                    table,
                    path,
                    source,
                }
            })?;
        }

        let marker = self.root.join(MARKER);
        File::create(&marker).map_err(|source| DatasetWriteError::PublishMarker {
            path: marker.clone(),
            source,
        })?;
        Ok(self.root)
    }
}

pub(crate) struct DatasetTableWriter {
    table: String,
    path: PathBuf,
    writer: ArrowWriter<File>,
}

impl DatasetTableWriter {
    pub(crate) fn write(&mut self, batch: &RecordBatch) -> Result<(), DatasetWriteError> {
        self.writer
            .write(batch)
            .map_err(|source| DatasetWriteError::WriteTable {
                table: self.table.clone(),
                path: self.path.clone(),
                source,
            })
    }

    pub(crate) fn finish(self) -> Result<(), DatasetWriteError> {
        self.writer
            .close()
            .map_err(|source| DatasetWriteError::CloseTable {
                table: self.table,
                path: self.path,
                source,
            })?;
        Ok(())
    }
}

fn prepare_target(target: &DatasetWriteTarget) -> Result<PathBuf, DatasetWriteError> {
    match fs::metadata(&target.path) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(DatasetWriteError::TargetNotDirectory {
                    path: target.path.clone(),
                });
            }
            let canonical = canonical_unicode(&target.path)?;
            let mut entries =
                fs::read_dir(&canonical).map_err(|source| DatasetWriteError::ReadTarget {
                    path: canonical.clone(),
                    source,
                })?;
            let nonempty = match entries.next() {
                Some(Ok(_)) => true,
                Some(Err(source)) => {
                    return Err(DatasetWriteError::ReadTarget {
                        path: canonical,
                        source,
                    });
                }
                None => false,
            };
            if nonempty && !target.overwrite {
                return Err(DatasetWriteError::TargetNotEmpty { path: canonical });
            }
            if nonempty {
                clear_directory(&canonical)?;
            }
            Ok(canonical)
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(&target.path).map_err(|source| DatasetWriteError::CreateTarget {
                path: target.path.clone(),
                source,
            })?;
            canonical_unicode(&target.path)
        }
        Err(source) => Err(DatasetWriteError::InspectTarget {
            path: target.path.clone(),
            source,
        }),
    }
}

fn canonical_unicode(path: &Path) -> Result<PathBuf, DatasetWriteError> {
    let canonical =
        dunce::canonicalize(path).map_err(|source| DatasetWriteError::CanonicalizeTarget {
            path: path.to_path_buf(),
            source,
        })?;
    if canonical.to_str().is_none() {
        return Err(DatasetWriteError::NonUnicodeTarget { path: canonical });
    }
    Ok(canonical)
}

fn clear_directory(root: &Path) -> Result<(), DatasetWriteError> {
    let entries = fs::read_dir(root).map_err(|source| DatasetWriteError::ReadTarget {
        path: root.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| DatasetWriteError::ReadTarget {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| {
            DatasetWriteError::InspectTargetEntry {
                path: path.clone(),
                source,
            }
        })?;
        let result = if metadata.file_type().is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        result.map_err(|source| DatasetWriteError::RemoveTargetEntry { path, source })?;
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum DatasetWriteError {
    #[error("failed to inspect Dataset target {path}")]
    InspectTarget {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Dataset target is not a directory: {path}")]
    TargetNotDirectory { path: PathBuf },
    #[error("failed to resolve Dataset target {path}")]
    CanonicalizeTarget {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Dataset target cannot be represented as native Unicode: {path:?}")]
    NonUnicodeTarget { path: PathBuf },
    #[error("failed to read Dataset target {path}")]
    ReadTarget {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "Dataset target is not empty; pass --overwrite-dataset to replace all contents: {path}"
    )]
    TargetNotEmpty { path: PathBuf },
    #[error("failed to create Dataset target {path}")]
    CreateTarget {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to inspect Dataset target entry {path}")]
    InspectTargetEntry {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to permanently remove Dataset target entry {path}")]
    RemoveTargetEntry {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to create Dataset tables directory {path}")]
    CreateTables {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid Dataset table name {name:?}")]
    InvalidTableName { name: String },
    #[error("duplicate Dataset table {name:?}")]
    DuplicateTable { name: String },
    #[error("Dataset table {table:?} has duplicate top-level column {column:?}")]
    DuplicateColumn { table: String, column: String },
    #[error("failed to create Dataset table {table:?} at {path}")]
    CreateTable {
        table: String,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to open Parquet writer for Dataset table {table:?} at {path}")]
    OpenTableWriter {
        table: String,
        path: PathBuf,
        #[source]
        source: ParquetError,
    },
    #[error("failed to write Dataset table {table:?} at {path}")]
    WriteTable {
        table: String,
        path: PathBuf,
        #[source]
        source: ParquetError,
    },
    #[error("failed to finish Dataset table {table:?} at {path}")]
    CloseTable {
        table: String,
        path: PathBuf,
        #[source]
        source: ParquetError,
    },
    #[error("failed to reopen Dataset table {table:?} at {path} for publication validation")]
    ValidateTableOpen {
        table: String,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to validate Dataset table {table:?} at {path} before publication")]
    ValidateTable {
        table: String,
        path: PathBuf,
        #[source]
        source: ParquetError,
    },
    #[error("failed to publish Dataset marker {path}")]
    PublishMarker {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use arrow_schema::{DataType, Field};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn duplicate_columns_fail_before_a_parquet_writer_is_created() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("dataset");
        let mut writer = DatasetWriter::begin(DatasetWriteTarget::new(&target, false)).unwrap();
        let schema = Arc::new(Schema::new(vec![
            Field::new("value", DataType::Int64, true),
            Field::new("value", DataType::Utf8, true),
        ]));

        assert!(matches!(
            writer.begin_table("facts", schema),
            Err(DatasetWriteError::DuplicateColumn { .. })
        ));
        assert!(!target.join(".kat-dataset").exists());
        assert!(!target.join("tables/facts.parquet").exists());
    }

    #[test]
    fn publication_failure_does_not_leave_a_recognizable_dataset() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("dataset");
        let writer = DatasetWriter::begin(DatasetWriteTarget::new(&target, false)).unwrap();
        fs::create_dir(target.join(MARKER)).unwrap();

        assert!(matches!(
            writer.finish(),
            Err(DatasetWriteError::PublishMarker { .. })
        ));
        assert!(!target.join(MARKER).is_file());
    }
}
