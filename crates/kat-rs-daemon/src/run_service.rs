use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use kat_rs_datasource::TraceDatasource;
use uuid::Uuid;

use crate::{
    api::{CreateRunRequest, EvidenceRecordDto, RunDto, RunEvidenceResponse, RunStatus},
    dataset_service::DatasetService,
    error::ApiError,
};

#[derive(Default)]
pub struct RunService {
    pack_root: PathBuf,
    store: Mutex<HashMap<String, StoredRun>>,
}

#[derive(Clone)]
struct StoredRun {
    run: RunDto,
    evidence: Vec<EvidenceRecordDto>,
}

impl RunService {
    pub fn new() -> Self {
        Self {
            pack_root: workspace_pack_root(),
            store: Mutex::default(),
        }
    }

    pub async fn create(
        &self,
        dataset_service: &DatasetService,
        request: CreateRunRequest,
    ) -> Result<RunDto, ApiError> {
        let resolved = dataset_service.resolve_existing(&request.dataset)?;
        let datasource = TraceDatasource::from_dataset(&resolved.path)
            .await
            .map_err(|error| ApiError::validation(format!("{error:#}")))?;
        let snapshot = crate::pack_runtime::load_snapshot(&self.pack_root, &request.pack_ref)?;
        let execution =
            crate::pack_runtime::execute_snapshot(&datasource, &snapshot, request.inputs).await?;
        let run_id = format!("run_{}", Uuid::now_v7().simple());
        let run = RunDto {
            run_id: run_id.clone(),
            status: RunStatus::Succeeded,
            dataset: resolved.dataset,
            pack_ref: request.pack_ref,
            outputs: execution.outputs,
            evidence_count: 0,
            diagnostics: Vec::new(),
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

fn workspace_pack_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("packs")
}
