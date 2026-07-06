use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;

use super::model::RunRecord;

#[derive(Default)]
pub struct RunStore {
    runs: RwLock<HashMap<String, Arc<RunRecord>>>,
}

impl RunStore {
    pub async fn insert(&self, run: RunRecord) -> Arc<RunRecord> {
        let run = Arc::new(run);
        self.runs
            .write()
            .await
            .insert(run.run_id.clone(), Arc::clone(&run));
        run
    }

    pub async fn get(&self, run_id: &str) -> Option<Arc<RunRecord>> {
        self.runs.read().await.get(run_id).cloned()
    }
}
