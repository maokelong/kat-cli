use crate::DatasetHandle;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use trace_query::{ParsedTraceQuerySession, ParsedTraceSource};

pub struct DatasetState {
    pub handle: DatasetHandle,
    pub sources: Arc<Vec<ParsedTraceSource>>,
    pub open_phase_elapsed_ms: BTreeMap<String, u64>,
    pub query_session: Mutex<Option<Arc<ParsedTraceQuerySession>>>,
}

impl DatasetState {
    pub fn new(
        handle: DatasetHandle,
        sources: Vec<ParsedTraceSource>,
        open_phase_elapsed_ms: BTreeMap<String, u64>,
    ) -> Self {
        Self {
            handle,
            sources: Arc::new(sources),
            open_phase_elapsed_ms,
            query_session: Mutex::new(None),
        }
    }
}
