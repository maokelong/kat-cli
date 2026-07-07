use std::{collections::HashMap, path::PathBuf, sync::Mutex};

use kat_rs_datasource::TraceDatasource;
use uuid::Uuid;

use crate::{
    api::{CreateRunRequest, EvidenceRecordDto, RunDto, RunEvidenceResponse, RunStatus},
    dataset_service::DatasetService,
    error::ApiError,
};

pub struct RunService {
    pack_root: Option<PathBuf>,
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
            pack_root: None,
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
        let pack_root = self.pack_root()?;
        let snapshot = crate::pack_runtime::load_snapshot(&pack_root, &request.pack_ref)?;
        let execution =
            crate::pack_runtime::execute_snapshot(&datasource, &snapshot, request.inputs).await?;
        let run_id = format!("run_{}", Uuid::now_v7().simple());
        let evidence_count = execution.evidence.len();
        let run = RunDto {
            run_id: run_id.clone(),
            status: RunStatus::Succeeded,
            dataset: resolved.dataset,
            pack_ref: request.pack_ref,
            outputs: execution.outputs,
            evidence_count,
            diagnostics: Vec::new(),
        };
        self.store.lock().expect("run store lock").insert(
            run_id,
            StoredRun {
                run: run.clone(),
                evidence: execution.evidence,
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

    fn pack_root(&self) -> Result<PathBuf, ApiError> {
        match &self.pack_root {
            Some(pack_root) => Ok(pack_root.clone()),
            None => std::env::current_dir()
                .map(|dir| dir.join("packs"))
                .map_err(|error| ApiError::internal(format!("failed to resolve cwd: {error}"))),
        }
    }

    #[cfg(test)]
    fn new_with_pack_root(pack_root: PathBuf) -> Self {
        Self {
            pack_root: Some(pack_root),
            store: Mutex::default(),
        }
    }
}

impl Default for RunService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        sync::{Mutex, OnceLock},
    };

    use tempfile::tempdir;

    use super::RunService;

    #[test]
    fn new_uses_runtime_current_dir_for_pack_root() {
        let _guard = current_dir_lock().lock().expect("current dir lock");
        let original = env::current_dir().expect("original cwd");
        let fixture = tempdir().expect("tempdir is created");
        let expected = fixture.path().join("packs");

        env::set_current_dir(fixture.path()).expect("cwd changes");
        let service = RunService::new();
        let actual = service.pack_root().expect("pack root resolves");
        env::set_current_dir(original).expect("cwd restores");

        assert_eq!(actual, expected);
    }

    #[test]
    fn new_with_pack_root_overrides_runtime_current_dir() {
        let _guard = current_dir_lock().lock().expect("current dir lock");
        let original = env::current_dir().expect("original cwd");
        let fixture = tempdir().expect("tempdir is created");
        let override_root = fixture.path().join("custom-packs");

        env::set_current_dir(fixture.path()).expect("cwd changes");
        let service = RunService::new_with_pack_root(override_root.clone());
        env::set_current_dir(original).expect("cwd restores");

        assert_eq!(
            service.pack_root().expect("pack root resolves"),
            override_root
        );
    }

    fn current_dir_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }
}
