//! KAT 唯一的内部 Datasource 与 Dataset Storage package。
//!
//! 当前切片开放 Dataset inspection、Dataset 写入以及 Deprecated Trace Streamer Import。
//! Trace Streamer 入口只服务预发布机制联调，不形成兼容承诺，并将在第一次正式发布前删除。

mod dataset;
mod dataset_writer;
mod decode;
mod formats;
mod json;
mod materializer;
mod mmap;
mod payload_value;
mod query;
mod record;
mod relational;
mod trace_streamer;

use std::{
    collections::HashSet,
    ffi::OsStr,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

use parquet::arrow::arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions};

pub use dataset::{
    DatasetLocator, DatasetResolution, DatasetStore, DatasetTableInfo, inspect_dataset_tables,
    write_derived_dataset_table,
};
pub use dataset_writer::DatasetWriteTarget;
pub use materializer::{
    HitraceImportError, ImportedHitrace, UnsupportedHitraceContent, import_hitrace,
    materialize_hitrace_dataset, materialize_langfuse_legacy_dataset,
};
pub use query::TraceDatasource;
pub use trace_streamer::{
    ImportedDataset, TraceStreamerImportError, import_deprecated_trace_streamer,
};

#[doc(hidden)]
pub mod relational_for_tests {
    pub fn descriptor_root_names() -> Vec<String> {
        crate::relational::descriptor::descriptor_root_names()
    }

    pub fn expansion_plan_table_names(root_messages: &[&str]) -> Vec<String> {
        crate::relational::plan::expansion_plan_for_roots(root_messages)
            .into_iter()
            .map(|item| item.output_table)
            .collect()
    }
}

#[allow(dead_code)]
pub(crate) mod proto {
    pub(crate) mod kat {
        pub(crate) mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }

        pub(crate) mod native_hook {
            include!(concat!(env!("OUT_DIR"), "/kat.native_hook.rs"));
        }

        pub(crate) mod cpu_data {
            include!(concat!(env!("OUT_DIR"), "/kat.cpu_data.rs"));
        }

        pub(crate) mod memory_data {
            include!(concat!(env!("OUT_DIR"), "/kat.memory_data.rs"));
        }

        pub(crate) mod process_data {
            include!(concat!(env!("OUT_DIR"), "/kat.process_data.rs"));
        }

        pub(crate) mod diskio_data {
            include!(concat!(env!("OUT_DIR"), "/kat.diskio_data.rs"));
        }

        pub(crate) mod network_data {
            include!(concat!(env!("OUT_DIR"), "/kat.network_data.rs"));
        }

        pub(crate) mod gpu_data {
            include!(concat!(env!("OUT_DIR"), "/kat.gpu_data.rs"));
        }
    }

    pub(crate) use kat::hitrace::{ProfilerPluginData, TracePluginResult};
    pub(crate) use kat::native_hook::{BatchNativeHookData, NativeHookConfig};
}

pub struct DatasetInspection {
    path: PathBuf,
    tables: Vec<TableInspection>,
}

impl DatasetInspection {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn tables(&self) -> &[TableInspection] {
        &self.tables
    }
}

pub struct TableInspection {
    name: String,
    columns: Vec<ColumnInspection>,
}

impl TableInspection {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn columns(&self) -> &[ColumnInspection] {
        &self.columns
    }
}

pub struct ColumnInspection {
    name: String,
    data_type: String,
    nullable: bool,
}

impl ColumnInspection {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn data_type(&self) -> &str {
        &self.data_type
    }

    pub fn nullable(&self) -> bool {
        self.nullable
    }
}

pub fn inspect_dataset(path: &Path) -> Result<DatasetInspection, DatasetInspectionError> {
    let root = canonical_unicode(path, "Dataset path")?;
    let root_metadata =
        fs::metadata(&root).map_err(|source| DatasetInspectionError::InspectPath {
            path: root.clone(),
            source,
        })?;
    if !root_metadata.is_dir() {
        return Err(DatasetInspectionError::NotDirectory { path: root });
    }
    validate_marker(&root)?;
    let candidates = scan_table_candidates(&root)?;
    let mut tables = Vec::with_capacity(candidates.len());
    for (name, path) in candidates {
        tables.push(read_table(name, path)?);
    }
    Ok(DatasetInspection { path: root, tables })
}

fn canonical_unicode(path: &Path, label: &'static str) -> Result<PathBuf, DatasetInspectionError> {
    let canonical =
        dunce::canonicalize(path).map_err(|source| DatasetInspectionError::Canonicalize {
            label,
            path: path.to_path_buf(),
            source,
        })?;
    if canonical.to_str().is_none() {
        return Err(DatasetInspectionError::NonUnicode {
            label,
            path: canonical,
        });
    }
    Ok(canonical)
}

fn validate_marker(root: &Path) -> Result<(), DatasetInspectionError> {
    let marker = root.join(".kat-dataset");
    let metadata =
        fs::symlink_metadata(&marker).map_err(|source| DatasetInspectionError::InspectMarker {
            path: marker.clone(),
            source,
        })?;
    if !metadata.file_type().is_file() || metadata.len() != 0 {
        return Err(DatasetInspectionError::InvalidMarker { path: marker });
    }
    Ok(())
}

fn scan_table_candidates(root: &Path) -> Result<Vec<(String, PathBuf)>, DatasetInspectionError> {
    let directory = root.join("tables");
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(DatasetInspectionError::InspectTablesDirectory {
                path: directory,
                source,
            });
        }
    };
    if !metadata.file_type().is_dir() {
        return Err(DatasetInspectionError::InvalidTablesDirectory { path: directory });
    }
    let entries =
        fs::read_dir(&directory).map_err(|source| DatasetInspectionError::ReadTablesDirectory {
            path: directory.clone(),
            source,
        })?;
    let mut entries = entries.collect::<Result<Vec<_>, _>>().map_err(|source| {
        DatasetInspectionError::EnumerateTablesDirectory {
            path: directory.clone(),
            source,
        }
    })?;
    entries.sort_by_key(fs::DirEntry::path);
    let mut candidates = Vec::new();
    for entry in entries {
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path).map_err(|source| {
            DatasetInspectionError::InspectTableEntry {
                path: entry_path.clone(),
                source,
            }
        })?;
        if !metadata.file_type().is_file() || entry_path.extension() != Some(OsStr::new("parquet"))
        {
            continue;
        }
        let Some(name) = entry_path.file_stem().and_then(OsStr::to_str) else {
            continue;
        };
        if !valid_table_name(name) {
            continue;
        }
        candidates.push((name.to_owned(), entry_path));
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(candidates)
}

pub(crate) fn valid_table_name(name: &str) -> bool {
    let valid = !name.is_empty()
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && name.as_bytes()[name.len() - 1].is_ascii_alphanumeric();
    valid && !is_windows_device_name(name)
}

fn is_windows_device_name(name: &str) -> bool {
    matches!(name, "con" | "prn" | "aux" | "nul")
        || (name.len() == 4
            && (name.starts_with("com") || name.starts_with("lpt"))
            && matches!(name.as_bytes()[3], b'1'..=b'9'))
}

fn read_table(name: String, path: PathBuf) -> Result<TableInspection, DatasetInspectionError> {
    let canonical = canonical_unicode(&path, "Dataset table path")?;
    let file = File::open(&canonical).map_err(|source| DatasetInspectionError::OpenTable {
        name: name.clone(),
        path: canonical.clone(),
        source,
    })?;
    let metadata =
        ArrowReaderMetadata::load(&file, ArrowReaderOptions::default()).map_err(|source| {
            DatasetInspectionError::ReadTableMetadata {
                name: name.clone(),
                path: canonical,
                source,
            }
        })?;
    let mut field_names = HashSet::new();
    let mut columns = Vec::new();
    for field in metadata.schema().fields() {
        if !field_names.insert(field.name().as_str()) {
            return Err(DatasetInspectionError::DuplicateColumn {
                table: name,
                column: field.name().clone(),
            });
        }
        columns.push(ColumnInspection {
            name: field.name().clone(),
            data_type: field.data_type().to_string(),
            nullable: field.is_nullable(),
        });
    }
    Ok(TableInspection { name, columns })
}

#[derive(Debug, thiserror::Error)]
pub enum DatasetInspectionError {
    #[error("failed to resolve {label} {path}")]
    Canonicalize {
        label: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{label} cannot be represented as native Unicode: {path:?}")]
    NonUnicode { label: &'static str, path: PathBuf },
    #[error("failed to inspect Dataset path {path}")]
    InspectPath {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Dataset path is not a directory: {path}")]
    NotDirectory { path: PathBuf },
    #[error("failed to inspect Dataset marker {path}")]
    InspectMarker {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Dataset marker must be an empty regular file: {path}")]
    InvalidMarker { path: PathBuf },
    #[error("failed to inspect Dataset tables directory {path}")]
    InspectTablesDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Dataset tables path must be a regular directory: {path}")]
    InvalidTablesDirectory { path: PathBuf },
    #[error("failed to read Dataset tables directory {path}")]
    ReadTablesDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed while enumerating Dataset tables directory {path}")]
    EnumerateTablesDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to inspect Dataset table entry {path}")]
    InspectTableEntry {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to open Dataset table {name:?} at {path}")]
    OpenTable {
        name: String,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to read Parquet metadata for Dataset table {name:?} at {path}")]
    ReadTableMetadata {
        name: String,
        path: PathBuf,
        #[source]
        source: parquet::errors::ParquetError,
    },
    #[error("Dataset table {table:?} has duplicate top-level column {column:?}")]
    DuplicateColumn { table: String, column: String },
}
