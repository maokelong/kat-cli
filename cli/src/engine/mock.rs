use crate::config::models::AtomicResources;
use crate::engine::{EngineInfo, QueryEnvelope, QueryStats, TraceInfo, TraceQueryEngine};
use anyhow::Result;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

pub struct MockTraceQueryEngine;

impl TraceQueryEngine for MockTraceQueryEngine {
    fn query(
        &self,
        atomic_id: &str,
        trace_path: &Path,
        _sql: &str,
        _resources: &AtomicResources,
    ) -> Result<QueryEnvelope> {
        let mut row = BTreeMap::new();
        row.insert("ok".to_string(), json!(1));
        Ok(QueryEnvelope {
            status: "ok".to_string(),
            atomic_id: atomic_id.to_string(),
            engine: EngineInfo {
                name: "mock".to_string(),
                version: "0.1.0".to_string(),
            },
            trace: TraceInfo {
                path: trace_path.display().to_string(),
            },
            rows: vec![row],
            artifacts: vec![],
            stats: QueryStats {
                rows_returned: 1,
                truncated: false,
            },
        })
    }
}
