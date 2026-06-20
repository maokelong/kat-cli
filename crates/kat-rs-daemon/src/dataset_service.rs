use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};

use kat_rs_datasource::{
    DatasetLocator, DatasetStore, materialize_hitrace_dataset, materialize_langfuse_legacy_dataset,
};
use serde_json::json;
use tokio::sync::Semaphore;

use crate::{
    api::{CreateDatasetRequest, DatasetDto, DatasetLocation, DatasetSourceInput, InputRole},
    error::ApiError,
    identity::resolve_input,
};

const DEFAULT_MAX_CONCURRENT_MATERIALIZATIONS: usize = 2;

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
        let dataset_store = dataset_store(&request.dataset)?;
        let dataset_name = request.dataset.name.clone();
        let resolution = dataset_store
            .resolve(&DatasetLocator::Name(dataset_name.clone()))
            .map_err(|error| ApiError::validation(format!("{error:#}")))?;
        let dataset_path = resolution.path;
        let dataset_directory = dataset_directory(&dataset_path)?;
        let load = dataset_load(request.input)?;
        let _permit = self
            .materialize_limiter
            .acquire()
            .await
            .map_err(|_| ApiError::internal("dataset materialize limiter closed"))?;

        ensure_dataset_target_absent(&dataset_path)?;
        materialize_dataset(load, &dataset_path).await?;

        Ok(DatasetDto {
            name: dataset_name,
            directory: dataset_directory.to_string_lossy().into_owned(),
            path: dataset_path.to_string_lossy().into_owned(),
        })
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
}

fn dataset_store(dataset: &DatasetLocation) -> Result<DatasetStore, ApiError> {
    if let Some(directory) = &dataset.directory {
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
