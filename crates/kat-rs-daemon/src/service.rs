use std::{path::PathBuf, sync::Arc};

use kat_datasource::TraceDatasource;
use serde_json::Value;
use time::OffsetDateTime;
use tokio::sync::Semaphore;

use crate::{
    api::{CreateDatasourceRequest, DatasourceDto, DatasourceSource, InputRole, QueryRequest},
    error::ApiError,
    identity::{DatasourceIdentityKey, InputIdentity, ResolvedInput, resolve_input},
    registry::{DatasourceEntry, DatasourceRegistry},
};

const DEFAULT_MAX_CONCURRENT_LOADS: usize = 2;
const MAX_LIST_LIMIT: usize = 500;

pub struct DatasourceService {
    registry: DatasourceRegistry<TraceDatasource>,
    load_limiter: Arc<Semaphore>,
}

impl DatasourceService {
    pub fn new(max_concurrent_loads: usize) -> Self {
        Self {
            registry: DatasourceRegistry::new(),
            load_limiter: Arc::new(Semaphore::new(max_concurrent_loads)),
        }
    }

    pub async fn create(
        &self,
        request: CreateDatasourceRequest,
    ) -> Result<(DatasourceDto, bool), ApiError> {
        let (identity, load) = match request {
            CreateDatasourceRequest::Hitrace { file } => {
                let input = resolve_input(InputRole::File, file)?;
                let path = input.path.clone();
                let identity = identity(DatasourceSource::Hitrace, vec![input]);
                let load = DatasourceLoad::Hitrace { path };

                (identity, load)
            }
            CreateDatasourceRequest::LangfuseLegacy {
                observations_file,
                traces_file,
            } => {
                let observations = resolve_input(InputRole::Observations, observations_file)?;
                let traces = resolve_input(InputRole::Traces, traces_file)?;
                let observations_path = observations.path.clone();
                let traces_path = traces.path.clone();
                let identity =
                    identity(DatasourceSource::LangfuseLegacy, vec![observations, traces]);
                let load = DatasourceLoad::LangfuseLegacy {
                    observations_path,
                    traces_path,
                };

                (identity, load)
            }
        };
        let load_limiter = Arc::clone(&self.load_limiter);
        let (entry, created) = self
            .registry
            .get_or_insert_with(identity, move || async move {
                load_datasource(load_limiter, load).await
            })
            .await?;

        Ok((entry.to_dto().await, created))
    }

    pub async fn list(&self, limit: usize, offset: usize) -> DatasourceList {
        let limit = limit.clamp(1, MAX_LIST_LIMIT);
        let (entries, total_items) = self.registry.list(limit, offset).await;
        let mut items = Vec::with_capacity(entries.len());

        for entry in entries {
            items.push(entry.to_dto().await);
        }

        DatasourceList {
            data: items,
            limit,
            offset,
            total_items,
        }
    }

    pub async fn get(&self, datasource_id: &str) -> Result<DatasourceDto, ApiError> {
        let entry = self.entry(datasource_id).await?;
        touch(&entry).await;

        Ok(entry.to_dto().await)
    }

    pub async fn delete(&self, datasource_id: &str) -> Result<(), ApiError> {
        if self.registry.remove(datasource_id).await {
            Ok(())
        } else {
            Err(ApiError::datasource_not_found(datasource_id))
        }
    }

    pub async fn query(
        &self,
        datasource_id: &str,
        request: QueryRequest,
    ) -> Result<Vec<Value>, ApiError> {
        let entry = self.entry(datasource_id).await?;
        touch(&entry).await;
        let rows = entry
            .datasource
            .query_json(&request.sql)
            .await
            .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;
        rows_as_array(rows)
    }

    async fn entry(
        &self,
        datasource_id: &str,
    ) -> Result<Arc<DatasourceEntry<TraceDatasource>>, ApiError> {
        self.registry
            .get(datasource_id)
            .await
            .ok_or_else(|| ApiError::datasource_not_found(datasource_id))
    }
}

impl Default for DatasourceService {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_CONCURRENT_LOADS)
    }
}

pub struct DatasourceList {
    pub data: Vec<DatasourceDto>,
    pub limit: usize,
    pub offset: usize,
    pub total_items: usize,
}

enum DatasourceLoad {
    Hitrace {
        path: PathBuf,
    },
    LangfuseLegacy {
        observations_path: PathBuf,
        traces_path: PathBuf,
    },
}

async fn load_datasource(
    load_limiter: Arc<Semaphore>,
    load: DatasourceLoad,
) -> Result<TraceDatasource, ApiError> {
    let _permit = load_limiter
        .acquire_owned()
        .await
        .map_err(|_| ApiError::internal("datasource load limiter closed"))?;

    match load {
        DatasourceLoad::Hitrace { path } => tokio::task::spawn_blocking(move || {
            TraceDatasource::from_hitrace(path)
                .map_err(|error| ApiError::validation(format!("{error:#}")))
        })
        .await
        .map_err(|error| {
            if error.is_panic() {
                ApiError::internal("hitrace datasource load panicked")
            } else {
                ApiError::internal("hitrace datasource load cancelled")
            }
        })?,
        DatasourceLoad::LangfuseLegacy {
            observations_path,
            traces_path,
        } => TraceDatasource::from_langfuse_legacy(observations_path, traces_path)
            .await
            .map_err(|error| ApiError::validation(format!("{error:#}"))),
    }
}

fn identity(source: DatasourceSource, inputs: Vec<ResolvedInput>) -> DatasourceIdentityKey {
    DatasourceIdentityKey::new(
        source,
        inputs
            .into_iter()
            .map(|input| input.to_identity())
            .collect::<Vec<InputIdentity>>(),
    )
}

async fn touch(entry: &DatasourceEntry<TraceDatasource>) {
    *entry.last_accessed_at.write().await = OffsetDateTime::now_utc();
}

fn rows_as_array(rows: Value) -> Result<Vec<Value>, ApiError> {
    match rows {
        Value::Array(rows) => Ok(rows),
        _ => Err(ApiError::internal(
            "datasource query returned a non-array JSON value",
        )),
    }
}
