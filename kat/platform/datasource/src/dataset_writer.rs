use std::{
    cell::RefCell,
    collections::HashSet,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    rc::Rc,
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

/// Dataset 写入目标及其对已有目录内容的显式处置授权。
#[derive(Clone, Debug)]
pub struct DatasetWriteTarget {
    path: PathBuf,
    existing_contents: ExistingContents,
    protected_paths: Vec<PathBuf>,
}

impl DatasetWriteTarget {
    /// 写入不存在或为空的目标目录，不授权删除任何已有内容。
    pub fn write_to_empty(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            existing_contents: ExistingContents::Reject,
            protected_paths: Vec::new(),
        }
    }

    /// 永久替换 resolved target 中的全部内容，包括 KAT 不识别的文件。
    ///
    /// 该授权没有备份、回滚或失败恢复；已有目标不是目录时仍会失败。
    pub fn permanently_replace_all_contents(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            existing_contents: ExistingContents::PermanentlyClear,
            protected_paths: Vec::new(),
        }
    }

    /// 拒绝会把指定路径包含在永久清空范围内的写入目标。
    pub fn protect_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.protected_paths.push(path.into());
        self
    }
}

#[derive(Clone, Copy, Debug)]
enum ExistingContents {
    Reject,
    PermanentlyClear,
}

pub(crate) struct DatasetWriter {
    root: PathBuf,
    tables: PathBuf,
    table_names: HashSet<String>,
}

/// 在目标目录内隔离构建候选 Dataset，成功时才替换旧内容并发布 marker。
pub(crate) struct DatasetPublication {
    root: PathBuf,
    staging: Option<tempfile::TempDir>,
    tables: DatasetTableFactory,
}

#[derive(Clone)]
pub(crate) struct DatasetTableFactory {
    tables: PathBuf,
    table_names: Rc<RefCell<HashSet<String>>>,
}

impl DatasetPublication {
    pub(crate) fn stage(target: DatasetWriteTarget) -> Result<Self, DatasetWriteError> {
        let root = prepare_staging_target(&target)?;
        let staging = tempfile::Builder::new()
            .prefix(".kat-staging-")
            .tempdir_in(&root)
            .map_err(|source| DatasetWriteError::CreateStaging {
                path: root.clone(),
                source,
            })?;
        let tables = staging.path().join("tables");
        fs::create_dir(&tables).map_err(|source| DatasetWriteError::CreateTables {
            path: tables.clone(),
            source,
        })?;
        Ok(Self {
            root,
            staging: Some(staging),
            tables: DatasetTableFactory {
                tables,
                table_names: Rc::new(RefCell::new(HashSet::new())),
            },
        })
    }

    pub(crate) fn table_factory(&self) -> DatasetTableFactory {
        self.tables.clone()
    }

    pub(crate) fn publish(mut self) -> Result<PathBuf, DatasetWriteError> {
        validate_tables(&self.tables.tables, &self.tables.table_names.borrow())?;

        // 一旦开始替换，先撤销旧 marker；后续任一步失败都不会留下可识别的半成品。
        invalidate_marker(&self.root)?;
        let staging_path = self
            .staging
            .as_ref()
            .expect("unpublished Dataset always owns staging")
            .path()
            .to_path_buf();
        clear_directory_except(&self.root, &staging_path)?;
        let staged_tables = staging_path.join("tables");
        let published_tables = self.root.join("tables");
        fs::rename(&staged_tables, &published_tables).map_err(|source| {
            DatasetWriteError::PublishTables {
                from: staged_tables,
                to: published_tables.clone(),
                source,
            }
        })?;
        self.staging
            .take()
            .expect("unpublished Dataset always owns staging")
            .close()
            .map_err(|source| DatasetWriteError::RemoveTargetEntry {
                path: staging_path,
                source,
            })?;
        validate_tables(&published_tables, &self.tables.table_names.borrow())?;
        publish_marker(&self.root)?;
        Ok(self.root.clone())
    }
}

impl Drop for DatasetPublication {
    fn drop(&mut self) {
        if let Some(staging) = self.staging.take() {
            let _ = staging.close();
        }
    }
}

impl DatasetTableFactory {
    pub(crate) fn begin_table(
        &self,
        name: &str,
        schema: Arc<Schema>,
    ) -> Result<DatasetTableWriter, DatasetWriteError> {
        begin_table(
            &self.tables,
            &mut self.table_names.borrow_mut(),
            name,
            schema,
            true,
        )
    }
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
        begin_table(&self.tables, &mut self.table_names, name, schema, false)
    }

    pub(crate) fn finish(self) -> Result<PathBuf, DatasetWriteError> {
        validate_tables(&self.tables, &self.table_names)?;
        publish_marker(&self.root)?;
        Ok(self.root)
    }
}

fn begin_table(
    tables: &Path,
    table_names: &mut HashSet<String>,
    name: &str,
    schema: Arc<Schema>,
    flush_after_write: bool,
) -> Result<DatasetTableWriter, DatasetWriteError> {
    if !valid_table_name(name) {
        return Err(DatasetWriteError::InvalidTableName {
            name: name.to_owned(),
        });
    }
    if !table_names.insert(name.to_owned()) {
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
    let path = tables.join(format!("{name}.parquet"));
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
        flush_after_write,
    })
}

fn validate_tables(tables: &Path, table_names: &HashSet<String>) -> Result<(), DatasetWriteError> {
    let mut names = table_names.iter().collect::<Vec<_>>();
    names.sort();
    for table in names {
        let path = tables.join(format!("{table}.parquet"));
        let file = File::open(&path).map_err(|source| DatasetWriteError::ValidateTableOpen {
            table: table.clone(),
            path: path.clone(),
            source,
        })?;
        ArrowReaderMetadata::load(&file, ArrowReaderOptions::default()).map_err(|source| {
            DatasetWriteError::ValidateTable {
                table: table.clone(),
                path,
                source,
            }
        })?;
    }
    Ok(())
}

fn publish_marker(root: &Path) -> Result<(), DatasetWriteError> {
    let marker = root.join(MARKER);
    File::create(&marker).map_err(|source| DatasetWriteError::PublishMarker {
        path: marker,
        source,
    })?;
    Ok(())
}

pub(crate) struct DatasetTableWriter {
    table: String,
    path: PathBuf,
    writer: ArrowWriter<File>,
    flush_after_write: bool,
}

impl DatasetTableWriter {
    pub(crate) fn write(&mut self, batch: &RecordBatch) -> Result<(), DatasetWriteError> {
        self.writer
            .write(batch)
            .map_err(|source| DatasetWriteError::WriteTable {
                table: self.table.clone(),
                path: self.path.clone(),
                source,
            })?;
        if self.flush_after_write {
            self.writer
                .flush()
                .map_err(|source| DatasetWriteError::WriteTable {
                    table: self.table.clone(),
                    path: self.path.clone(),
                    source,
                })?;
        }
        Ok(())
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
            protect_paths_from_clear(target, &canonical)?;
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
            if nonempty && matches!(target.existing_contents, ExistingContents::Reject) {
                return Err(DatasetWriteError::TargetNotEmpty { path: canonical });
            }
            if nonempty {
                clear_existing_target(&canonical)?;
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

fn prepare_staging_target(target: &DatasetWriteTarget) -> Result<PathBuf, DatasetWriteError> {
    match fs::metadata(&target.path) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(DatasetWriteError::TargetNotDirectory {
                    path: target.path.clone(),
                });
            }
            let canonical = canonical_unicode(&target.path)?;
            protect_paths_from_clear(target, &canonical)?;
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
            if nonempty && matches!(target.existing_contents, ExistingContents::Reject) {
                return Err(DatasetWriteError::TargetNotEmpty { path: canonical });
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

fn protect_paths_from_clear(
    target: &DatasetWriteTarget,
    canonical_target: &Path,
) -> Result<(), DatasetWriteError> {
    if !matches!(target.existing_contents, ExistingContents::PermanentlyClear) {
        return Ok(());
    }
    for path in &target.protected_paths {
        let protected = dunce::canonicalize(path).map_err(|source| {
            DatasetWriteError::CanonicalizeProtectedPath {
                path: path.clone(),
                source,
            }
        })?;
        if protected.starts_with(canonical_target) {
            return Err(DatasetWriteError::ProtectedPathInsideTarget {
                target: canonical_target.to_path_buf(),
                protected,
            });
        }
    }
    Ok(())
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

fn clear_existing_target(root: &Path) -> Result<(), DatasetWriteError> {
    clear_existing_target_with(root, clear_directory)
}

fn clear_existing_target_with(
    root: &Path,
    clear: impl FnOnce(&Path) -> Result<(), DatasetWriteError>,
) -> Result<(), DatasetWriteError> {
    // 覆盖一旦开始便先撤销识别标记，使后续破坏式失败只留下不可识别候选。
    invalidate_marker(root)?;
    clear(root)
}

fn invalidate_marker(root: &Path) -> Result<(), DatasetWriteError> {
    let marker = root.join(MARKER);
    match fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.file_type().is_file() => {
            fs::remove_file(&marker).map_err(|source| DatasetWriteError::RemoveTargetEntry {
                path: marker,
                source,
            })
        }
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(DatasetWriteError::InspectTargetEntry {
            path: marker,
            source,
        }),
    }
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

fn clear_directory_except(root: &Path, preserved: &Path) -> Result<(), DatasetWriteError> {
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
        if path == preserved {
            continue;
        }
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
    #[error("failed to resolve protected path {path}")]
    CanonicalizeProtectedPath {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Dataset target {target} would permanently delete protected path {protected}")]
    ProtectedPathInsideTarget { target: PathBuf, protected: PathBuf },
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
    #[error("failed to create Dataset staging directory inside {path}")]
    CreateStaging {
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
    #[error("failed to move staged Dataset tables from {from} to {to}")]
    PublishTables {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: io::Error,
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
