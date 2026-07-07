use std::{collections::HashMap, sync::Mutex};

use serde_json::json;
use uuid::Uuid;

use crate::{
    api::{
        CreateRunRequest, EvidenceRecordDto, RunDto, RunEvidenceResponse, RunOutputDto, RunStatus,
    },
    dataset_service::DatasetService,
    error::ApiError,
};

#[derive(Default)]
pub struct RunService {
    store: Mutex<HashMap<String, StoredRun>>,
}

#[derive(Clone)]
struct StoredRun {
    run: RunDto,
    evidence: Vec<EvidenceRecordDto>,
}

impl RunService {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create(
        &self,
        dataset_service: &DatasetService,
        request: CreateRunRequest,
    ) -> Result<RunDto, ApiError> {
        let resolved = dataset_service.resolve_existing(&request.dataset)?;
        let run_id = format!("run_{}", Uuid::now_v7().simple());
        let run = RunDto {
            run_id: run_id.clone(),
            status: RunStatus::Failed,
            dataset: resolved.dataset,
            pack_ref: request.pack_ref,
            outputs: [(
                "mvp".to_string(),
                RunOutputDto {
                    kind: "diagnostic".to_string(),
                    name: "pack runtime not wired".to_string(),
                    row_count: None,
                },
            )]
            .into(),
            evidence_count: 0,
            diagnostics: vec![json!({ "reason": "pack runtime not wired" }).to_string()],
        };
        self.store.lock().expect("run store lock").insert(
            run_id,
            StoredRun {
                run: run.clone(),
                evidence: Vec::new(),
            },
        );

        Ok(run)
    }

    pub fn get(&self, run_id: &str) -> Result<RunDto, ApiError> {
        self.store
            .lock()
            .expect("run store lock")
            .get(run_id)
            .map(|run| run.run.clone())
            .ok_or_else(|| ApiError::run_not_found(run_id))
    }

    pub fn evidence(&self, run_id: &str) -> Result<RunEvidenceResponse, ApiError> {
        let store = self.store.lock().expect("run store lock");
        let stored = store
            .get(run_id)
            .ok_or_else(|| ApiError::run_not_found(run_id))?;

        Ok(RunEvidenceResponse {
            run: stored.run.clone(),
            evidence: stored.evidence.clone(),
        })
    }
}
