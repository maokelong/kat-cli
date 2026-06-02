use crate::DatasetHandle;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use trace_query::{ParsedTraceQuerySession, ParsedTraceSource};

pub struct DatasetState {
    pub handle: DatasetHandle,
    pub sources: Arc<Vec<ParsedTraceSource>>,
    pub cache_dir: Option<PathBuf>,
    pub cache_hit: bool,
    pub open_phase_elapsed_ms: BTreeMap<String, u64>,
    pub query_session: Mutex<Option<Arc<ParsedTraceQuerySession>>>,
}

impl DatasetState {
    pub fn new(
        handle: DatasetHandle,
        sources: Vec<ParsedTraceSource>,
        cache_dir: Option<PathBuf>,
        cache_hit: bool,
        open_phase_elapsed_ms: BTreeMap<String, u64>,
    ) -> Self {
        Self {
            handle,
            sources: Arc::new(sources),
            cache_dir,
            cache_hit,
            open_phase_elapsed_ms,
            query_session: Mutex::new(None),
        }
    }
}
