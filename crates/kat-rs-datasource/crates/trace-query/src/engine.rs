use crate::query_parsed_trace;
use crate::{
    OpenOptions, QueryRequest, QueryResult, TableInspection, TraceEngineError, TraceHandle,
    TraceInput, TraceInspection, TraceQueryEngine, TraceResult, SCHEMA_VERSION,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use trace_model::ParsedTrace;
use trace_parser::parse_trace_file;

#[derive(Default)]
pub struct HtraceDataFusionEngine {
    traces: Mutex<HashMap<String, ParsedTrace>>,
}

impl HtraceDataFusionEngine {
    pub fn new() -> Self {
        Self::default()
    }

    fn get_trace(&self, handle: &TraceHandle) -> TraceResult<ParsedTrace> {
        self.traces
            .lock()
            .map_err(|_| TraceEngineError::Engine("trace cache lock poisoned".to_string()))?
            .get(&handle.trace_id)
            .cloned()
            .ok_or_else(|| {
                TraceEngineError::Engine(format!("unknown trace handle {}", handle.trace_id))
            })
    }
}

#[async_trait::async_trait]
impl TraceQueryEngine for HtraceDataFusionEngine {
    async fn open(&self, input: TraceInput, _options: OpenOptions) -> TraceResult<TraceHandle> {
        let parsed = parse_trace_file(&input.path)?;
        let handle = TraceHandle {
            trace_id: parsed.trace_id.clone(),
            path: input.path,
        };
        self.traces
            .lock()
            .map_err(|_| TraceEngineError::Engine("trace cache lock poisoned".to_string()))?
            .insert(handle.trace_id.clone(), parsed);
        Ok(handle)
    }

    async fn inspect(&self, handle: &TraceHandle) -> TraceResult<TraceInspection> {
        let parsed = self.get_trace(handle)?;
        let tables = parsed
            .tables
            .batches()
            .into_iter()
            .map(|(name, batch)| {
                (
                    name.to_string(),
                    TableInspection {
                        available: true,
                        row_count: batch.num_rows(),
                        reason: None,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();

        Ok(TraceInspection {
            schema_version: SCHEMA_VERSION.to_string(),
            trace_id: parsed.trace_id,
            path: handle.path.clone(),
            start_ts: parsed.start_ts,
            end_ts: parsed.end_ts,
            clock_domain: parsed.clock_domain,
            tables,
        })
    }

    async fn query(&self, handle: &TraceHandle, request: QueryRequest) -> TraceResult<QueryResult> {
        let parsed = self.get_trace(handle)?;
        query_parsed_trace(&parsed, request).await
    }

    async fn close(&self, handle: TraceHandle) -> TraceResult<()> {
        self.traces
            .lock()
            .map_err(|_| TraceEngineError::Engine("trace cache lock poisoned".to_string()))?
            .remove(&handle.trace_id);
        Ok(())
    }
}
