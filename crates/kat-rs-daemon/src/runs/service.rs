use std::sync::Arc;

use uuid::Uuid;

use crate::{
    api::{CreateRunRequest, DatasetDto},
    error::ApiError,
};

use super::{model::RunRecord, store::RunStore};

#[derive(Default)]
pub struct RunService {
    store: RunStore,
}

impl RunService {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create_placeholder(
        &self,
        request: CreateRunRequest,
        dataset: DatasetDto,
    ) -> Arc<RunRecord> {
        let run_id = Uuid::now_v7().simple().to_string();
        let run = RunRecord::failed_placeholder(run_id, request.pack_ref, dataset, request.inputs);

        self.store.insert(run).await
    }

    pub async fn get(&self, run_id: &str) -> Result<Arc<RunRecord>, ApiError> {
        self.store
            .get(run_id)
            .await
            .ok_or_else(|| ApiError::validation(format!("run not found: {run_id}")))
    }
}
