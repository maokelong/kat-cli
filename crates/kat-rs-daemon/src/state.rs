use std::sync::Arc;

use tokio::sync::Notify;

use crate::{dataset_service::DatasetService, service::DatasourceService};

#[derive(Clone)]
pub struct AppState {
    pub dataset_service: Arc<DatasetService>,
    pub datasource_service: Arc<DatasourceService>,
    pub shutdown: Arc<Notify>,
}

impl AppState {
    pub fn new(max_concurrent_loads: usize) -> Self {
        Self {
            dataset_service: Arc::new(DatasetService::new(max_concurrent_loads)),
            datasource_service: Arc::new(DatasourceService::new(max_concurrent_loads)),
            shutdown: Arc::new(Notify::new()),
        }
    }

    pub fn new_for_tests() -> Self {
        Self::new(1)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new_for_tests()
    }
}
