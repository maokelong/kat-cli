use std::sync::Arc;

use kat_rs_datasource::TraceDatasource;
use serde_json::Value;
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

        publish_inputs(&mut state, inputs)?;
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

        let snapshot_digest = format!("{},{},{}", manifest.digest, pack.digest, entry_flow.digest);
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
            .ok_or_else(|| ApiError::validation(format!("run not found: {run_id}")))
    }
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
