//! `kat/platform` 新架构唯一的内部 Datasource 与 Dataset Storage 边界。
//!
//! 当前切片只开放只读 Dataset inspection，并未把 Dataset Storage 从后续内置 Datasource
//! 中拆出。`crates/kat-rs-datasource` 属于旧应用代码；它不被 `kat/platform` 依赖，也不是
//! 这一新架构边界的权威实现。

use std::{
    collections::HashSet,
    ffi::OsStr,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

use parquet::arrow::arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions};

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

fn valid_table_name(name: &str) -> bool {
    let valid = !name.is_empty()
        && name.split('_').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
        && name.as_bytes()[0].is_ascii_lowercase();
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_schema::{DataType, Field, Schema};
    use parquet::arrow::ArrowWriter;

    use super::*;

    fn dataset(root: &Path) -> PathBuf {
        let path = root.join("dataset");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join(".kat-dataset"), []).unwrap();
        path
    }

    fn parquet(path: &Path, fields: Vec<Field>) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = File::create(path).unwrap();
        ArrowWriter::try_new(file, Arc::new(Schema::new(fields)), None)
            .unwrap()
            .close()
            .unwrap();
    }

    fn error(result: Result<DatasetInspection, DatasetInspectionError>) -> DatasetInspectionError {
        match result {
            Ok(_) => panic!("expected Dataset inspection failure"),
            Err(error) => error,
        }
    }

    #[test]
    fn inspection_returns_canonical_path_sorted_tables_and_ordered_columns() {
        let temporary = tempfile::tempdir().unwrap();
        let dataset = dataset(temporary.path());
        parquet(
            &dataset.join("tables/zeta.parquet"),
            vec![Field::new("value", DataType::UInt64, false)],
        );
        parquet(
            &dataset.join("tables/alpha.parquet"),
            vec![
                Field::new("id", DataType::Int64, false),
                Field::new("label", DataType::Utf8, true),
            ],
        );
        fs::write(dataset.join("tables/bad-name.parquet"), "ignored").unwrap();
        fs::write(dataset.join("tables/readme.txt"), "ignored").unwrap();
        fs::create_dir(dataset.join("tables/nested.parquet")).unwrap();

        let inspection = inspect_dataset(&dataset).expect("inspect Dataset");

        assert_eq!(inspection.path(), dunce::canonicalize(dataset).unwrap());
        assert_eq!(
            inspection
                .tables()
                .iter()
                .map(TableInspection::name)
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        let columns = inspection.tables()[0].columns();
        assert_eq!(columns[0].name(), "id");
        assert_eq!(columns[0].data_type(), "Int64");
        assert!(!columns[0].nullable());
        assert_eq!(columns[1].name(), "label");
        assert_eq!(columns[1].data_type(), "Utf8");
        assert!(columns[1].nullable());
    }

    #[test]
    fn missing_tables_directory_is_a_valid_empty_dataset() {
        let temporary = tempfile::tempdir().unwrap();
        let dataset = dataset(temporary.path());

        let inspection = inspect_dataset(&dataset).unwrap();

        assert!(inspection.tables().is_empty());
        assert!(!dataset.join("tables").exists());
    }

    #[test]
    fn marker_must_be_an_empty_regular_file() {
        for marker in ["missing", "non-empty", "directory"] {
            let temporary = tempfile::tempdir().unwrap();
            let dataset = temporary.path().join("dataset");
            fs::create_dir(&dataset).unwrap();
            match marker {
                "non-empty" => fs::write(dataset.join(".kat-dataset"), "content").unwrap(),
                "directory" => fs::create_dir(dataset.join(".kat-dataset")).unwrap(),
                _ => {}
            }

            let failure = error(inspect_dataset(&dataset));

            assert!(matches!(
                failure,
                DatasetInspectionError::InspectMarker { .. }
                    | DatasetInspectionError::InvalidMarker { .. }
            ));
        }
    }

    #[test]
    fn reserved_tables_path_must_be_an_ordinary_directory() {
        let temporary = tempfile::tempdir().unwrap();
        let dataset = dataset(temporary.path());
        fs::write(dataset.join("tables"), "not a directory").unwrap();

        assert!(matches!(
            error(inspect_dataset(&dataset)),
            DatasetInspectionError::InvalidTablesDirectory { .. }
        ));
    }

    #[test]
    fn corrupted_managed_tables_fail_in_name_order() {
        let temporary = tempfile::tempdir().unwrap();
        let dataset = dataset(temporary.path());
        fs::create_dir(dataset.join("tables")).unwrap();
        fs::write(dataset.join("tables/zeta.parquet"), "broken").unwrap();
        fs::write(dataset.join("tables/alpha.parquet"), "broken").unwrap();

        assert!(matches!(
            error(inspect_dataset(&dataset)),
            DatasetInspectionError::ReadTableMetadata { name, .. } if name == "alpha"
        ));
    }

    #[test]
    fn duplicate_top_level_columns_are_rejected() {
        let temporary = tempfile::tempdir().unwrap();
        let dataset = dataset(temporary.path());
        parquet(
            &dataset.join("tables/duplicate.parquet"),
            vec![
                Field::new("value", DataType::Int64, false),
                Field::new("value", DataType::Utf8, true),
            ],
        );

        assert!(matches!(
            error(inspect_dataset(&dataset)),
            DatasetInspectionError::DuplicateColumn { table, column }
                if table == "duplicate" && column == "value"
        ));
    }
}
