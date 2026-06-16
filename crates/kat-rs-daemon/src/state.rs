use std::sync::Arc;

use tokio::sync::Notify;

use crate::service::DatasourceService;

#[derive(Clone)]
pub struct AppState {
    pub datasource_service: Arc<DatasourceService>,
    pub shutdown: Arc<Notify>,
}

impl AppState {
    pub fn new(max_concurrent_loads: usize) -> Self {
        Self {
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
