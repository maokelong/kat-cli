use std::{
    collections::HashSet,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

use arrow_schema::Schema;
use parquet::arrow::arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions};
use serde::{Deserialize, Serialize};

use crate::valid_table_name;

const DATASET_MARKER: &str = ".kat-dataset";
const BINDINGS_FILE: &str = "bindings.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DatasetBindingKind {
    External,
    Materialized,
}

impl DatasetBindingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::External => "external",
            Self::Materialized => "materialized",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DatasetInspection {
    path: PathBuf,
    sources: Vec<SourceInspection>,
}

impl DatasetInspection {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn sources(&self) -> &[SourceInspection] {
        &self.sources
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceInspection {
    External {
        pack: String,
        source: String,
    },
    Materialized {
        pack: String,
        source: String,
        tables: Vec<TableInspection>,
    },
}

impl SourceInspection {
    pub fn pack(&self) -> &str {
        match self {
            Self::External { pack, .. } | Self::Materialized { pack, .. } => pack,
        }
    }

    pub fn source(&self) -> &str {
        match self {
            Self::External { source, .. } | Self::Materialized { source, .. } => source,
        }
    }

    pub fn kind(&self) -> DatasetBindingKind {
        match self {
            Self::External { .. } => DatasetBindingKind::External,
            Self::Materialized { .. } => DatasetBindingKind::Materialized,
        }
    }

    pub fn tables(&self) -> Option<&[TableInspection]> {
        match self {
            Self::External { .. } => None,
            Self::Materialized { tables, .. } => Some(tables),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ColumnInspection {
    name: String,
    #[serde(rename = "type")]
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

/// 交给 Python Runtime 的唯一 Dataset 投影。
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResolvedDataset {
    path: PathBuf,
    sources: Vec<ResolvedSource>,
}

impl ResolvedDataset {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn sources(&self) -> &[ResolvedSource] {
        &self.sources
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolvedSource {
    External {
        pack: String,
        source: String,
        arguments: Vec<String>,
        working_directory: PathBuf,
    },
    Materialized {
        pack: String,
        source: String,
        arguments: Vec<String>,
        working_directory: PathBuf,
        tables: Vec<ResolvedTable>,
    },
}

impl ResolvedSource {
    pub fn pack(&self) -> &str {
        match self {
            Self::External { pack, .. } | Self::Materialized { pack, .. } => pack,
        }
    }

    pub fn source(&self) -> &str {
        match self {
            Self::External { source, .. } | Self::Materialized { source, .. } => source,
        }
    }

    pub fn kind(&self) -> DatasetBindingKind {
        match self {
            Self::External { .. } => DatasetBindingKind::External,
            Self::Materialized { .. } => DatasetBindingKind::Materialized,
        }
    }

    pub fn arguments(&self) -> Option<&[String]> {
        match self {
            Self::External { arguments, .. } | Self::Materialized { arguments, .. } => {
                Some(arguments)
            }
        }
    }

    pub fn working_directory(&self) -> Option<&Path> {
        match self {
            Self::External {
                working_directory, ..
            }
            | Self::Materialized {
                working_directory, ..
            } => Some(working_directory),
        }
    }

    pub fn tables(&self) -> Option<&[ResolvedTable]> {
        match self {
            Self::External { .. } => None,
            Self::Materialized { tables, .. } => Some(tables),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResolvedTable {
    name: String,
    path: PathBuf,
}

impl ResolvedTable {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetTargetInspection {
    path: PathBuf,
    exists: bool,
    binding: Option<DatasetBindingKind>,
    resolved_binding: Option<ResolvedSource>,
}

impl DatasetTargetInspection {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.exists
    }

    pub fn binding(&self) -> Option<DatasetBindingKind> {
        self.binding
    }

    /// 返回当前同名 Binding 的完整 Runtime 投影；Materialize 可据此重放 Source recipe。
    pub fn resolved_binding(&self) -> Option<&ResolvedSource> {
        self.resolved_binding.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DatasetMetadata {
    bindings: Vec<Binding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Binding {
    External {
        pack: String,
        source: String,
        arguments: Vec<String>,
        working_directory: PathBuf,
    },
    Materialized {
        pack: String,
        source: String,
        arguments: Vec<String>,
        working_directory: PathBuf,
        tables: Vec<String>,
    },
}

impl Binding {
    fn pack(&self) -> &str {
        match self {
            Self::External { pack, .. } | Self::Materialized { pack, .. } => pack,
        }
    }

    fn source(&self) -> &str {
        match self {
            Self::External { source, .. } | Self::Materialized { source, .. } => source,
        }
    }
}

struct ValidatedDataset {
    path: PathBuf,
    metadata: DatasetMetadata,
    sources: Vec<ValidatedSource>,
}

enum ValidatedSource {
    External {
        pack: String,
        source: String,
        arguments: Vec<String>,
        working_directory: PathBuf,
    },
    Materialized {
        pack: String,
        source: String,
        arguments: Vec<String>,
        working_directory: PathBuf,
        tables: Vec<ValidatedTable>,
    },
}

impl ValidatedSource {
    fn pack(&self) -> &str {
        match self {
            Self::External { pack, .. } | Self::Materialized { pack, .. } => pack,
        }
    }

    fn source(&self) -> &str {
        match self {
            Self::External { source, .. } | Self::Materialized { source, .. } => source,
        }
    }

    fn into_resolved(self) -> ResolvedSource {
        match self {
            Self::External {
                pack,
                source,
                arguments,
                working_directory,
            } => ResolvedSource::External {
                pack,
                source,
                arguments,
                working_directory,
            },
            Self::Materialized {
                pack,
                source,
                arguments,
                working_directory,
                tables,
            } => ResolvedSource::Materialized {
                pack,
                source,
                arguments,
                working_directory,
                tables: tables
                    .into_iter()
                    .map(|table| ResolvedTable {
                        name: table.name,
                        path: table.path,
                    })
                    .collect(),
            },
        }
    }
}

struct ValidatedTable {
    name: String,
    path: PathBuf,
    columns: Vec<ColumnInspection>,
}

pub fn inspect_dataset(path: &Path) -> Result<DatasetInspection, DatasetInspectionError> {
    let validated = validate_dataset(path)?;
    let sources = validated
        .sources
        .into_iter()
        .map(|source| match source {
            ValidatedSource::External { pack, source, .. } => {
                SourceInspection::External { pack, source }
            }
            ValidatedSource::Materialized {
                pack,
                source,
                tables,
                ..
            } => SourceInspection::Materialized {
                pack,
                source,
                tables: tables
                    .into_iter()
                    .map(|table| TableInspection {
                        name: table.name,
                        columns: table.columns,
                    })
                    .collect(),
            },
        })
        .collect();

    Ok(DatasetInspection {
        path: validated.path,
        sources,
    })
}

pub fn resolve_dataset(path: &Path) -> Result<ResolvedDataset, DatasetInspectionError> {
    let validated = validate_dataset(path)?;
    let sources = validated
        .sources
        .into_iter()
        .map(ValidatedSource::into_resolved)
        .collect();

    Ok(ResolvedDataset {
        path: validated.path,
        sources,
    })
}

pub fn inspect_dataset_target(
    path: &Path,
    pack: &str,
    source: &str,
) -> Result<DatasetTargetInspection, DatasetMutationError> {
    validate_binding_identity(pack, source).map_err(DatasetMutationError::InvalidBinding)?;
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let validated = validate_dataset(path).map_err(DatasetMutationError::InspectDataset)?;
            let resolved_binding = validated
                .sources
                .into_iter()
                .find(|binding| binding.pack() == pack && binding.source() == source)
                .map(ValidatedSource::into_resolved);
            let binding = resolved_binding.as_ref().map(ResolvedSource::kind);
            Ok(DatasetTargetInspection {
                path: validated.path,
                exists: true,
                binding,
                resolved_binding,
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let absolute = absolute_unicode(path)?;
            Ok(DatasetTargetInspection {
                path: absolute,
                exists: false,
                binding: None,
                resolved_binding: None,
            })
        }
        Err(source) => Err(DatasetMutationError::InspectTarget {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub fn write_external_binding(
    dataset_path: &Path,
    pack: &str,
    source: &str,
    arguments: Vec<String>,
    working_directory: &Path,
    replace: bool,
) -> Result<ResolvedDataset, DatasetMutationError> {
    validate_binding_identity(pack, source).map_err(DatasetMutationError::InvalidBinding)?;
    validate_working_directory(working_directory).map_err(DatasetMutationError::InvalidBinding)?;
    let target = inspect_dataset_target(dataset_path, pack, source)?;
    reject_collision(&target, pack, source, replace)?;

    let (root, mut metadata) = open_or_create_dataset(dataset_path, target.exists)?;
    if target.binding.is_some() {
        metadata
            .bindings
            .retain(|binding| binding.pack() != pack || binding.source() != source);
    }
    remove_source_space_if_present(&root, pack, source)?;
    metadata.bindings.push(Binding::External {
        pack: pack.to_owned(),
        source: source.to_owned(),
        arguments,
        working_directory: working_directory.to_path_buf(),
    });
    write_metadata(&root, &mut metadata)?;
    resolve_dataset(&root).map_err(DatasetMutationError::InspectDataset)
}

pub struct MaterializedSourcePublication<'a> {
    pub pack: &'a str,
    pub source: &'a str,
    pub arguments: Vec<String>,
    pub working_directory: &'a Path,
    pub table_names: &'a [String],
    pub export_directory: &'a Path,
    pub replace: bool,
}

pub fn publish_materialized_source(
    dataset_path: &Path,
    publication: MaterializedSourcePublication<'_>,
) -> Result<ResolvedDataset, DatasetMutationError> {
    let MaterializedSourcePublication {
        pack,
        source,
        arguments,
        working_directory,
        table_names,
        export_directory,
        replace,
    } = publication;
    validate_binding_identity(pack, source).map_err(DatasetMutationError::InvalidBinding)?;
    validate_working_directory(working_directory).map_err(DatasetMutationError::InvalidBinding)?;
    let table_names = validate_materialized_table_names(table_names)
        .map_err(DatasetMutationError::InvalidBinding)?;
    let exports = validate_exports(export_directory, &table_names)?;
    let target = inspect_dataset_target(dataset_path, pack, source)?;
    reject_collision(&target, pack, source, replace)?;

    let (root, mut metadata) = open_or_create_dataset(dataset_path, target.exists)?;
    if target.binding.is_some() {
        metadata
            .bindings
            .retain(|binding| binding.pack() != pack || binding.source() != source);
    }
    remove_source_space_if_present(&root, pack, source)?;
    let tables_directory = root.join("sources").join(pack).join(source).join("tables");
    fs::create_dir_all(&tables_directory).map_err(|source| {
        DatasetMutationError::CreateSourceDirectory {
            path: tables_directory.clone(),
            source,
        }
    })?;
    for (name, export) in exports {
        let destination = tables_directory.join(format!("{name}.parquet"));
        fs::copy(&export, &destination).map_err(|source| DatasetMutationError::PublishTable {
            table: name,
            from: export,
            to: destination,
            source,
        })?;
    }
    metadata.bindings.push(Binding::Materialized {
        pack: pack.to_owned(),
        source: source.to_owned(),
        arguments,
        working_directory: working_directory.to_path_buf(),
        tables: table_names,
    });
    write_metadata(&root, &mut metadata)?;
    resolve_dataset(&root).map_err(DatasetMutationError::InspectDataset)
}

fn validate_dataset(path: &Path) -> Result<ValidatedDataset, DatasetInspectionError> {
    let target_metadata =
        fs::metadata(path).map_err(|source| DatasetInspectionError::InspectPath {
            path: path.to_path_buf(),
            source,
        })?;
    if !target_metadata.is_dir() {
        return Err(DatasetInspectionError::NotDirectory {
            path: path.to_path_buf(),
        });
    }
    let root = canonical_unicode(path, "Dataset path")?;
    validate_marker(&root)?;
    let mut metadata = read_metadata(&root)?;
    let mut identities = HashSet::new();
    for binding in &mut metadata.bindings {
        validate_binding_identity(binding.pack(), binding.source())?;
        if !identities.insert((binding.pack().to_owned(), binding.source().to_owned())) {
            return Err(DatasetInspectionError::DuplicateBinding {
                pack: binding.pack().to_owned(),
                source_name: binding.source().to_owned(),
            });
        }
        match binding {
            Binding::External {
                working_directory, ..
            }
            | Binding::Materialized {
                working_directory, ..
            } => validate_working_directory(working_directory)?,
        }
        if let Binding::Materialized { tables, .. } = binding {
            *tables = validate_materialized_table_names(tables)?;
        }
    }
    sort_bindings(&mut metadata.bindings);

    let mut sources = Vec::with_capacity(metadata.bindings.len());
    for binding in &metadata.bindings {
        match binding {
            Binding::External {
                pack,
                source,
                arguments,
                working_directory,
            } => sources.push(ValidatedSource::External {
                pack: pack.clone(),
                source: source.clone(),
                arguments: arguments.clone(),
                working_directory: working_directory.clone(),
            }),
            Binding::Materialized {
                pack,
                source,
                arguments,
                working_directory,
                tables,
            } => sources.push(ValidatedSource::Materialized {
                pack: pack.clone(),
                source: source.clone(),
                arguments: arguments.clone(),
                working_directory: working_directory.clone(),
                tables: validate_materialized_tables(&root, pack, source, tables)?,
            }),
        }
    }

    Ok(ValidatedDataset {
        path: root,
        metadata,
        sources,
    })
}

fn validate_marker(root: &Path) -> Result<(), DatasetInspectionError> {
    let marker = root.join(DATASET_MARKER);
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

fn read_metadata(root: &Path) -> Result<DatasetMetadata, DatasetInspectionError> {
    let path = root.join(BINDINGS_FILE);
    let file_metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            if root.join("catalog.json").exists() || root.join("tables").exists() {
                return Err(DatasetInspectionError::LegacyDataset {
                    path: root.to_path_buf(),
                });
            }
            return Err(DatasetInspectionError::InspectBindings { path, source });
        }
        Err(source) => return Err(DatasetInspectionError::InspectBindings { path, source }),
    };
    if !file_metadata.file_type().is_file() {
        return Err(DatasetInspectionError::InvalidBindingsFile { path });
    }
    let contents =
        fs::read_to_string(&path).map_err(|source| DatasetInspectionError::ReadBindings {
            path: path.clone(),
            source,
        })?;
    serde_json::from_str(&contents)
        .map_err(|source| DatasetInspectionError::ParseBindings { path, source })
}

fn validate_materialized_tables(
    root: &Path,
    pack: &str,
    source: &str,
    names: &[String],
) -> Result<Vec<ValidatedTable>, DatasetInspectionError> {
    let tables_root = root.join("sources").join(pack).join(source).join("tables");
    validate_directory_chain(root, &tables_root)?;
    let canonical_tables_root = canonical_unicode(&tables_root, "Source tables path")?;
    if !canonical_tables_root.starts_with(root) {
        return Err(DatasetInspectionError::TablePathEscapesSource {
            table: "<source>".to_owned(),
            path: canonical_tables_root,
        });
    }

    names
        .iter()
        .map(|name| validate_materialized_table(name, &tables_root, &canonical_tables_root))
        .collect()
}

fn validate_materialized_table(
    name: &str,
    tables_root: &Path,
    canonical_tables_root: &Path,
) -> Result<ValidatedTable, DatasetInspectionError> {
    let file_path = tables_root.join(format!("{name}.parquet"));
    let metadata = match optional_metadata(&file_path)? {
        Some(metadata) => metadata,
        None => {
            return Err(DatasetInspectionError::MissingTable {
                table: name.to_owned(),
                source_directory: tables_root.to_path_buf(),
            });
        }
    };
    if !metadata.file_type().is_file() {
        return Err(DatasetInspectionError::InvalidTableStorage {
            table: name.to_owned(),
            path: file_path,
        });
    }
    let path = canonical_table_path(name, &file_path, canonical_tables_root)?;
    let schema = read_parquet_schema(name, &path)?;
    let columns = inspect_columns(name, &schema)?;
    Ok(ValidatedTable {
        name: name.to_owned(),
        path,
        columns,
    })
}

fn validate_directory_chain(root: &Path, target: &Path) -> Result<(), DatasetInspectionError> {
    let relative = target
        .strip_prefix(root)
        .expect("Source directory is constructed beneath Dataset root");
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(|source| {
            DatasetInspectionError::InspectSourceDirectory {
                path: current.clone(),
                source,
            }
        })?;
        if !metadata.file_type().is_dir() {
            return Err(DatasetInspectionError::InvalidSourceDirectory { path: current });
        }
    }
    Ok(())
}

fn optional_metadata(path: &Path) -> Result<Option<fs::Metadata>, DatasetInspectionError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(DatasetInspectionError::InspectTableStorage {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn canonical_table_path(
    table: &str,
    path: &Path,
    source_root: &Path,
) -> Result<PathBuf, DatasetInspectionError> {
    let canonical = canonical_unicode(path, "Dataset table path")?;
    if !canonical.starts_with(source_root) {
        return Err(DatasetInspectionError::TablePathEscapesSource {
            table: table.to_owned(),
            path: canonical,
        });
    }
    Ok(canonical)
}

fn read_parquet_schema(table: &str, path: &Path) -> Result<Schema, DatasetInspectionError> {
    let file = File::open(path).map_err(|source| DatasetInspectionError::OpenTable {
        table: table.to_owned(),
        path: path.to_path_buf(),
        source,
    })?;
    let metadata =
        ArrowReaderMetadata::load(&file, ArrowReaderOptions::default()).map_err(|source| {
            DatasetInspectionError::ReadTableMetadata {
                table: table.to_owned(),
                path: path.to_path_buf(),
                source,
            }
        })?;
    let schema = metadata.schema().as_ref().clone();
    inspect_columns(table, &schema)?;
    Ok(schema)
}

fn inspect_columns(
    table: &str,
    schema: &Schema,
) -> Result<Vec<ColumnInspection>, DatasetInspectionError> {
    let mut names = HashSet::new();
    let mut columns = Vec::with_capacity(schema.fields().len());
    for field in schema.fields() {
        if !names.insert(field.name().as_str()) {
            return Err(DatasetInspectionError::DuplicateColumn {
                table: table.to_owned(),
                column: field.name().clone(),
            });
        }
        columns.push(ColumnInspection {
            name: field.name().clone(),
            data_type: field.data_type().to_string(),
            nullable: field.is_nullable(),
        });
    }
    Ok(columns)
}

fn validate_binding_identity(pack: &str, source: &str) -> Result<(), DatasetInspectionError> {
    if !valid_pack_name(pack) {
        return Err(DatasetInspectionError::InvalidPackName {
            name: pack.to_owned(),
        });
    }
    if !valid_table_name(source) || matches!(source, "dataset" | "information_schema") {
        return Err(DatasetInspectionError::InvalidSourceName {
            name: source.to_owned(),
        });
    }
    Ok(())
}

fn valid_pack_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('-').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
        && !is_windows_device_name(name)
}

fn is_windows_device_name(name: &str) -> bool {
    matches!(name, "con" | "prn" | "aux" | "nul")
        || (name.len() == 4
            && (name.starts_with("com") || name.starts_with("lpt"))
            && matches!(name.as_bytes()[3], b'1'..=b'9'))
}

fn validate_working_directory(path: &Path) -> Result<(), DatasetInspectionError> {
    if !path.is_absolute() {
        return Err(DatasetInspectionError::RelativeWorkingDirectory {
            path: path.to_path_buf(),
        });
    }
    if path.to_str().is_none() {
        return Err(DatasetInspectionError::NonUnicode {
            label: "Binding working directory",
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn validate_materialized_table_names(
    names: &[String],
) -> Result<Vec<String>, DatasetInspectionError> {
    if names.is_empty() {
        return Err(DatasetInspectionError::EmptyMaterializedTables);
    }
    let mut sorted = names.to_vec();
    sorted.sort();
    for name in &sorted {
        if !valid_table_name(name) {
            return Err(DatasetInspectionError::InvalidTableName { name: name.clone() });
        }
    }
    for pair in sorted.windows(2) {
        if pair[0] == pair[1] {
            return Err(DatasetInspectionError::DuplicateTableName {
                name: pair[0].clone(),
            });
        }
    }
    Ok(sorted)
}

fn sort_bindings(bindings: &mut [Binding]) {
    bindings
        .sort_by(|left, right| (left.pack(), left.source()).cmp(&(right.pack(), right.source())));
}

fn absolute_unicode(path: &Path) -> Result<PathBuf, DatasetMutationError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(DatasetMutationError::CurrentDirectory)?
            .join(path)
    };
    if absolute.to_str().is_none() {
        return Err(DatasetMutationError::NonUnicodeTarget { path: absolute });
    }
    Ok(absolute)
}

fn reject_collision(
    target: &DatasetTargetInspection,
    pack: &str,
    source: &str,
    replace: bool,
) -> Result<(), DatasetMutationError> {
    if let Some(kind) = target.binding
        && !replace
    {
        return Err(DatasetMutationError::BindingExists {
            pack: pack.to_owned(),
            source_name: source.to_owned(),
            kind,
        });
    }
    Ok(())
}

fn open_or_create_dataset(
    path: &Path,
    exists: bool,
) -> Result<(PathBuf, DatasetMetadata), DatasetMutationError> {
    if exists {
        let validated = validate_dataset(path).map_err(DatasetMutationError::InspectDataset)?;
        return Ok((validated.path, validated.metadata));
    }
    fs::create_dir_all(path).map_err(|source| DatasetMutationError::CreateDataset {
        path: path.to_path_buf(),
        source,
    })?;
    let root =
        canonical_unicode(path, "Dataset path").map_err(DatasetMutationError::InvalidBinding)?;
    fs::write(root.join(BINDINGS_FILE), b"{\"bindings\":[]}").map_err(|source| {
        DatasetMutationError::WriteBindings {
            path: root.join(BINDINGS_FILE),
            source,
        }
    })?;
    fs::write(root.join(DATASET_MARKER), []).map_err(|source| {
        DatasetMutationError::WriteMarker {
            path: root.join(DATASET_MARKER),
            source,
        }
    })?;
    Ok((
        root,
        DatasetMetadata {
            bindings: Vec::new(),
        },
    ))
}

fn remove_source_space_if_present(
    root: &Path,
    pack: &str,
    source_name: &str,
) -> Result<(), DatasetMutationError> {
    let path = root.join("sources").join(pack).join(source_name);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(DatasetMutationError::InspectSourceSpace { path, source });
        }
    };
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(&path)
            .map_err(|source| DatasetMutationError::RemoveSourceSpace { path, source })
    } else if metadata.file_type().is_file() {
        fs::remove_file(&path)
            .map_err(|source| DatasetMutationError::RemoveSourceSpace { path, source })
    } else {
        Err(DatasetMutationError::InvalidSourceSpace { path })
    }
}

fn write_metadata(root: &Path, metadata: &mut DatasetMetadata) -> Result<(), DatasetMutationError> {
    sort_bindings(&mut metadata.bindings);
    for binding in &mut metadata.bindings {
        if let Binding::Materialized { tables, .. } = binding {
            tables.sort();
        }
    }
    let path = root.join(BINDINGS_FILE);
    let file = File::create(&path).map_err(|source| DatasetMutationError::WriteBindings {
        path: path.clone(),
        source,
    })?;
    serde_json::to_writer_pretty(file, metadata)
        .map_err(|source| DatasetMutationError::SerializeBindings { path, source })
}

fn validate_exports(
    directory: &Path,
    table_names: &[String],
) -> Result<Vec<(String, PathBuf)>, DatasetMutationError> {
    let metadata = fs::symlink_metadata(directory).map_err(|source| {
        DatasetMutationError::InspectExportDirectory {
            path: directory.to_path_buf(),
            source,
        }
    })?;
    if !metadata.file_type().is_dir() {
        return Err(DatasetMutationError::InvalidExportDirectory {
            path: directory.to_path_buf(),
        });
    }
    let root = canonical_unicode(directory, "materialize export directory")
        .map_err(DatasetMutationError::InvalidBinding)?;
    let mut exports = Vec::with_capacity(table_names.len());
    for name in table_names {
        let path = root.join(format!("{name}.parquet"));
        let metadata = fs::symlink_metadata(&path).map_err(|source| {
            DatasetMutationError::InspectExportTable {
                table: name.clone(),
                path: path.clone(),
                source,
            }
        })?;
        if !metadata.file_type().is_file() {
            return Err(DatasetMutationError::InvalidExportTable {
                table: name.clone(),
                path,
            });
        }
        read_parquet_schema(name, &path).map_err(DatasetMutationError::InspectDataset)?;
        exports.push((name.clone(), path));
    }
    Ok(exports)
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

#[derive(Debug, thiserror::Error)]
pub enum DatasetInspectionError {
    #[error("failed to inspect Dataset path {path}")]
    InspectPath {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Dataset path must be an ordinary directory: {path}")]
    NotDirectory { path: PathBuf },
    #[error("failed to resolve {label} {path}")]
    Canonicalize {
        label: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{label} cannot be represented as native Unicode: {path:?}")]
    NonUnicode { label: &'static str, path: PathBuf },
    #[error("failed to inspect Dataset marker {path}")]
    InspectMarker {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Dataset marker must be an empty ordinary file: {path}")]
    InvalidMarker { path: PathBuf },
    #[error(
        "legacy flat Dataset is unsupported; rebuild it from its source and configuration: {path}"
    )]
    LegacyDataset { path: PathBuf },
    #[error("failed to inspect Dataset Binding metadata {path}")]
    InspectBindings {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Dataset Binding metadata must be an ordinary file: {path}")]
    InvalidBindingsFile { path: PathBuf },
    #[error("failed to read Dataset Binding metadata as UTF-8: {path}")]
    ReadBindings {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse Dataset Binding metadata {path}")]
    ParseBindings {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid PACK identity in Dataset Binding: {name:?}")]
    InvalidPackName { name: String },
    #[error("invalid Source identity in Dataset Binding: {name:?}")]
    InvalidSourceName { name: String },
    #[error("duplicate Dataset Binding for {pack}/{source_name}")]
    DuplicateBinding { pack: String, source_name: String },
    #[error("Binding working directory must be absolute: {path}")]
    RelativeWorkingDirectory { path: PathBuf },
    #[error("Materialized Source must contain at least one table")]
    EmptyMaterializedTables,
    #[error("invalid Materialized Source table name: {name:?}")]
    InvalidTableName { name: String },
    #[error("duplicate Materialized Source table name: {name:?}")]
    DuplicateTableName { name: String },
    #[error("failed to inspect managed Source directory {path}")]
    InspectSourceDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("managed Source path must be an ordinary directory: {path}")]
    InvalidSourceDirectory { path: PathBuf },
    #[error("failed to inspect managed table storage {path}")]
    InspectTableStorage {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Materialized table {table:?} is missing beneath {source_directory}")]
    MissingTable {
        table: String,
        source_directory: PathBuf,
    },
    #[error("Materialized table {table:?} is not an ordinary Parquet file: {path}")]
    InvalidTableStorage { table: String, path: PathBuf },
    #[error("Materialized table {table:?} path escapes its Source space: {path}")]
    TablePathEscapesSource { table: String, path: PathBuf },
    #[error("failed to open Materialized table {table:?} at {path}")]
    OpenTable {
        table: String,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to read Parquet metadata for Materialized table {table:?} at {path}")]
    ReadTableMetadata {
        table: String,
        path: PathBuf,
        #[source]
        source: parquet::errors::ParquetError,
    },
    #[error("Materialized table {table:?} has duplicate top-level column {column:?}")]
    DuplicateColumn { table: String, column: String },
}

#[derive(Debug, thiserror::Error)]
pub enum DatasetMutationError {
    #[error("invalid Dataset Binding")]
    InvalidBinding(#[source] DatasetInspectionError),
    #[error("failed to inspect existing Dataset")]
    InspectDataset(#[source] DatasetInspectionError),
    #[error("failed to inspect Dataset target {path}")]
    InspectTarget {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to read current directory")]
    CurrentDirectory(#[source] io::Error),
    #[error("Dataset target cannot be represented as native Unicode: {path:?}")]
    NonUnicodeTarget { path: PathBuf },
    #[error("Dataset already binds {pack}/{source_name} as {kind:?}; pass replace explicitly")]
    BindingExists {
        pack: String,
        source_name: String,
        kind: DatasetBindingKind,
    },
    #[error("failed to create Dataset {path}")]
    CreateDataset {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to publish Dataset marker {path}")]
    WriteMarker {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to write Dataset Binding metadata {path}")]
    WriteBindings {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to serialize Dataset Binding metadata {path}")]
    SerializeBindings {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to inspect old Source space {path}")]
    InspectSourceSpace {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("old Source space is not an ordinary file or directory: {path}")]
    InvalidSourceSpace { path: PathBuf },
    #[error("failed to remove old Source space {path}")]
    RemoveSourceSpace {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to inspect materialize export directory {path}")]
    InspectExportDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("materialize export path must be an ordinary directory: {path}")]
    InvalidExportDirectory { path: PathBuf },
    #[error("failed to inspect materialize export for table {table:?}: {path}")]
    InspectExportTable {
        table: String,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("materialize export for table {table:?} must be an ordinary Parquet file: {path}")]
    InvalidExportTable { table: String, path: PathBuf },
    #[error("failed to create Materialized Source directory {path}")]
    CreateSourceDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to publish Materialized table {table:?} from {from} to {to}")]
    PublishTable {
        table: String,
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: io::Error,
    },
}
