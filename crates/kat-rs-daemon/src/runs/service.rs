use std::{fs, path::Path, sync::Arc};

use kat_rs_datasource::TraceDatasource;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    api::{CreateRunRequest, DatasetDto},
    error::ApiError,
};

use super::{
    model::RunRecord,
    operators::{ExecutionState, build_brief_sections, execute_flow},
    resources::ResourceRoot,
    store::RunStore,
};

#[derive(Default)]
pub struct RunService {
    store: RunStore,
}

impl RunService {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create(
        &self,
        request: CreateRunRequest,
        dataset: DatasetDto,
    ) -> Result<Arc<RunRecord>, ApiError> {
        let CreateRunRequest {
            pack_ref, inputs, ..
        } = request;
        let run_id = Uuid::now_v7().simple().to_string();
        let datasource = TraceDatasource::from_dataset(&dataset.path)
            .await
            .map_err(|error| ApiError::validation(format!("{error:#}")))?;
        let resource_root = ResourceRoot::new("resources");
        let manifest = resource_root.load_manifest()?;
        let pack = resource_root.load_pack(&manifest.value, &pack_ref)?;
        let entry_flow = resource_root.load_entry_flow(&pack)?;
        let mut state = ExecutionState::new(datasource);

        state.record_resource_digest(manifest.digest.clone());
        state.record_resource_digest(pack.digest.clone());
        state.record_resource_digest(entry_flow.digest.clone());
        publish_inputs(&mut state, inputs.clone())?;
        publish_constants(&mut state, &entry_flow.value.constants)?;
        execute_flow(
            &resource_root,
            &manifest.value,
            &pack.value,
            &entry_flow.value,
            &mut state,
        )
        .await?;
        build_brief_sections(&resource_root, &pack, &mut state).await?;

        let snapshot_digest =
            stable_run_snapshot_digest(&dataset, &inputs, &state.resource_digests)?;
        let run = RunRecord::completed(
            run_id,
            pack_ref,
            dataset,
            snapshot_digest,
            state.steps,
            state.diagnostics,
            state.evidence,
            state.brief_sections,
        );

        Ok(self.store.insert(run).await)
    }

    pub async fn get(&self, run_id: &str) -> Result<Arc<RunRecord>, ApiError> {
        self.store
            .get(run_id)
            .await
            .ok_or_else(|| ApiError::run_not_found(run_id))
    }
}

fn stable_run_snapshot_digest(
    dataset: &DatasetDto,
    inputs: &std::collections::BTreeMap<String, Value>,
    resource_digests: &[String],
) -> Result<String, ApiError> {
    let mut resource_digests = resource_digests.to_vec();
    resource_digests.sort();
    resource_digests.dedup();

    let snapshot = json!({
        "dataset": dataset,
        "datasetCatalogDigest": file_digest(Path::new(&dataset.path).join("catalog.json"))?,
        "inputs": inputs,
        "resources": resource_digests,
    });
    let bytes = serde_json::to_vec(&snapshot).map_err(|error| {
        ApiError::internal(format!("failed to serialize run snapshot: {error}"))
    })?;

    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

fn file_digest(path: impl AsRef<Path>) -> Result<String, ApiError> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|error| {
        ApiError::validation(format!(
            "failed to read dataset catalog for run snapshot {}: {error}",
            path.display()
        ))
    })?;

    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

fn publish_inputs(
    state: &mut ExecutionState,
    inputs: std::collections::BTreeMap<String, Value>,
) -> Result<(), ApiError> {
    for (slot, value) in inputs {
        state.context.publish_scalar(&slot, value, "run.inputs")?;
    }

    Ok(())
}

fn publish_constants(state: &mut ExecutionState, constants: &Value) -> Result<(), ApiError> {
    let constants = constants.as_object().ok_or_else(|| {
        ApiError::validation("entry flow constants must be an object when present")
    })?;

    for (slot, value) in constants {
        state
            .context
            .publish_scalar(slot, value.clone(), "entry_flow.constants")?;
    }

    Ok(())
}
