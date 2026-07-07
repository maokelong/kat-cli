use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};

use kat_rs_datasource::{
    DatasetLocator, DatasetStore, TraceDatasource, inspect_dataset_tables,
    materialize_hitrace_dataset, materialize_langfuse_legacy_dataset,
    materialize_sqlite_pack_demo_dataset,
};
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use crate::{
    api::{
        CreateDatasetRequest, DatasetDto, DatasetInspectResponse, DatasetLocation,
        DatasetQueryRequest, DatasetSourceInput, DatasetTableDto, InputRole,
    },
    error::ApiError,
    identity::resolve_input,
};

const DEFAULT_MAX_CONCURRENT_MATERIALIZATIONS: usize = 2;
const MAX_LIST_LIMIT: usize = 500;

pub struct DatasetService {
    materialize_limiter: Arc<Semaphore>,
}

impl DatasetService {
    pub fn new(max_concurrent_materializations: usize) -> Self {
        Self {
            materialize_limiter: Arc::new(Semaphore::new(max_concurrent_materializations)),
        }
    }

    pub async fn create(&self, request: CreateDatasetRequest) -> Result<DatasetDto, ApiError> {
        let resolved = self.resolve_location(&request.dataset)?;
        let load = dataset_load(request.input)?;
        let _permit = self
            .materialize_limiter
            .acquire()
            .await
            .map_err(|_| ApiError::internal("dataset materialize limiter closed"))?;

        ensure_dataset_target_absent(&resolved.path)?;
        materialize_dataset(load, &resolved.path).await?;

        Ok(resolved.dataset)
    }

    pub fn list(
        &self,
        directory: Option<String>,
        limit: usize,
        offset: usize,
    ) -> Result<DatasetList, ApiError> {
        let limit = limit.clamp(1, MAX_LIST_LIMIT);
        let dataset_store = dataset_store_from_directory(directory.as_deref())?;
        let datasets_dir = dataset_store.datasets_dir().to_path_buf();
        let mut datasets = list_dataset_dirs(&datasets_dir)?;
        datasets.sort_by(|left, right| left.name.cmp(&right.name));

        let total_items = datasets.len();
        let data = datasets.into_iter().skip(offset).take(limit).collect();

        Ok(DatasetList {
            data,
            limit,
            offset,
            total_items,
        })
    }

    pub fn inspect(&self, dataset: DatasetLocation) -> Result<DatasetInspectResponse, ApiError> {
        let resolved = self.resolve_location(&dataset)?;
        ensure_dataset_exists(&resolved.path)?;
        let tables = inspect_dataset_tables(&resolved.path)
            .map_err(|error| ApiError::validation(format!("{error:#}")))?
            .into_iter()
            .map(|table| DatasetTableDto {
                kind: table.kind.to_string(),
                name: table.name,
                path: table.path,
                size_bytes: table.size_bytes,
            })
            .collect();

        Ok(DatasetInspectResponse {
            dataset: resolved.dataset,
            tables,
        })
    }

    pub fn delete(&self, dataset: DatasetLocation) -> Result<(), ApiError> {
        let resolved = self.resolve_location(&dataset)?;
        ensure_dataset_exists(&resolved.path)?;
        fs::remove_dir_all(&resolved.path).map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                ApiError::dataset_not_found(&resolved.path)
            } else {
                ApiError::validation(format!(
                    "failed to delete dataset {}: {error}",
                    resolved.path.display()
                ))
            }
        })
    }

    pub async fn query(
        &self,
        request: DatasetQueryRequest,
    ) -> Result<(DatasetDto, Vec<Value>), ApiError> {
        let resolved = self.resolve_location(&request.dataset)?;
        ensure_dataset_exists(&resolved.path)?;
        let datasource = TraceDatasource::from_dataset(&resolved.path)
            .await
            .map_err(|error| ApiError::validation(format!("{error:#}")))?;
        let rows = datasource
            .query_json(&request.sql)
            .await
            .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;

        Ok((resolved.dataset, rows_as_array(rows)?))
    }

    pub fn resolve_location(&self, dataset: &DatasetLocation) -> Result<ResolvedDataset, ApiError> {
        resolve_dataset(dataset)
    }

    pub fn resolve_existing(&self, dataset: &DatasetLocation) -> Result<ResolvedDataset, ApiError> {
        let resolved = self.resolve_location(dataset)?;
        ensure_dataset_exists(&resolved.path)?;
        Ok(resolved)
    }
}

impl Default for DatasetService {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_CONCURRENT_MATERIALIZATIONS)
    }
}

enum DatasetLoad {
    Hitrace {
        path: PathBuf,
    },
    LangfuseLegacy {
        observations_path: PathBuf,
        traces_path: PathBuf,
    },
    Sqlite {
        path: PathBuf,
    },
}

pub struct ResolvedDataset {
    pub dataset: DatasetDto,
    pub path: PathBuf,
}

pub struct DatasetList {
    pub data: Vec<DatasetDto>,
    pub limit: usize,
    pub offset: usize,
    pub total_items: usize,
}

fn resolve_dataset(dataset: &DatasetLocation) -> Result<ResolvedDataset, ApiError> {
    let dataset_store = dataset_store(dataset)?;
    let dataset_name = dataset.name.clone();
    let resolution = dataset_store
        .resolve(&DatasetLocator::Name(dataset_name.clone()))
        .map_err(|error| ApiError::validation(format!("{error:#}")))?;
    let dataset_path = resolution.path;
    let dataset_directory = dataset_directory(&dataset_path)?;

    Ok(ResolvedDataset {
        dataset: DatasetDto {
            name: dataset_name,
            directory: dataset_directory.to_string_lossy().into_owned(),
            path: dataset_path.to_string_lossy().into_owned(),
        },
        path: dataset_path,
    })
}

fn dataset_store(dataset: &DatasetLocation) -> Result<DatasetStore, ApiError> {
    dataset_store_from_directory(dataset.directory.as_deref())
}

fn dataset_store_from_directory(directory: Option<&str>) -> Result<DatasetStore, ApiError> {
    if let Some(directory) = directory {
        let directory = PathBuf::from(directory);
        if !directory.is_absolute() {
            return Err(ApiError::validation(format!(
                "dataset directory must be an absolute path: {}",
                directory.display()
            )));
        }

        return Ok(DatasetStore::from_datasets_dir(directory));
    }

    DatasetStore::default_from_env().map_err(|error| ApiError::validation(format!("{error:#}")))
}

fn list_dataset_dirs(datasets_dir: &Path) -> Result<Vec<DatasetDto>, ApiError> {
    let entries = match fs::read_dir(datasets_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(ApiError::validation(format!(
                "failed to list dataset directory {}: {error}",
                datasets_dir.display()
            )));
        }
    };
    let mut datasets = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| {
            ApiError::validation(format!(
                "failed to read dataset directory entry in {}: {error}",
                datasets_dir.display()
            ))
        })?;
        let file_type = entry.file_type().map_err(|error| {
            ApiError::validation(format!(
                "failed to inspect dataset directory entry {}: {error}",
                entry.path().display()
            ))
        })?;
        if !file_type.is_dir() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let dataset = DatasetLocation {
            name,
            directory: Some(datasets_dir.to_string_lossy().into_owned()),
        };
        let resolved = resolve_dataset(&dataset)?;
        datasets.push(resolved.dataset);
    }

    Ok(datasets)
}

fn dataset_directory(dataset_path: &Path) -> Result<PathBuf, ApiError> {
    dataset_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .ok_or_else(|| ApiError::internal("resolved dataset path is missing parent directory"))
}

fn dataset_load(input: DatasetSourceInput) -> Result<DatasetLoad, ApiError> {
    match input {
        DatasetSourceInput::Hitrace { file } => {
            let input = resolve_input(InputRole::File, file)?;

            Ok(DatasetLoad::Hitrace { path: input.path })
        }
        DatasetSourceInput::LangfuseLegacy {
            observations_file,
            traces_file,
        } => {
            let observations = resolve_input(InputRole::Observations, observations_file)?;
            let traces = resolve_input(InputRole::Traces, traces_file)?;

            Ok(DatasetLoad::LangfuseLegacy {
                observations_path: observations.path,
                traces_path: traces.path,
            })
        }
        DatasetSourceInput::Sqlite { file } => {
            let input = resolve_input(InputRole::File, file)?;

            Ok(DatasetLoad::Sqlite { path: input.path })
        }
    }
}

async fn materialize_dataset(load: DatasetLoad, dataset_path: &Path) -> Result<(), ApiError> {
    let result = match load {
        DatasetLoad::Hitrace { path } => materialize_hitrace_dataset(path, dataset_path).await,
        DatasetLoad::LangfuseLegacy {
            observations_path,
            traces_path,
        } => {
            materialize_langfuse_legacy_dataset(observations_path, traces_path, dataset_path).await
        }
        DatasetLoad::Sqlite { path } => {
            materialize_sqlite_pack_demo_dataset(path, dataset_path).await
        }
    };

    result.map_err(|error| map_materialize_error(error, dataset_path))
}

fn ensure_dataset_target_absent(dataset_path: &Path) -> Result<(), ApiError> {
    match fs::symlink_metadata(dataset_path) {
        Ok(_) => Err(dataset_exists(dataset_path)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ApiError::validation(format!(
            "failed to inspect dataset target {}: {error}",
            dataset_path.display()
        ))),
    }
}

fn ensure_dataset_exists(dataset_path: &Path) -> Result<(), ApiError> {
    match fs::symlink_metadata(dataset_path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(ApiError::validation(format!(
            "dataset path is not a directory: {}",
            dataset_path.display()
        ))),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Err(ApiError::dataset_not_found(dataset_path))
        }
        Err(error) => Err(ApiError::validation(format!(
            "failed to inspect dataset target {}: {error}",
            dataset_path.display()
        ))),
    }
}

fn map_materialize_error(error: anyhow::Error, dataset_path: &Path) -> ApiError {
    let message = format!("{error:#}");
    if message.contains("dataset target already exists") {
        dataset_exists(dataset_path)
    } else {
        ApiError::validation(message)
    }
}

fn dataset_exists(dataset_path: &Path) -> ApiError {
    ApiError::conflict(
        "dataset already exists",
        Some(json!({ "path": dataset_path.to_string_lossy() })),
    )
}

fn rows_as_array(rows: Value) -> Result<Vec<Value>, ApiError> {
    match rows {
        Value::Array(rows) => Ok(rows),
        _ => Err(ApiError::internal(
            "dataset query returned a non-array JSON value",
        )),
    }
}
