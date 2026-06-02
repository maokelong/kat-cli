use crate::json::batches_to_query_result;
use crate::logical_source::ParsedTraceSource;
use crate::registry::{register_parsed_trace_sources, register_parsed_traces};
use crate::{QueryRequest, QueryResult, TraceEngineError, TraceResult};
use datafusion::prelude::SessionContext;
use trace_model::ParsedTrace;

pub struct ParsedTraceQuerySession {
    ctx: SessionContext,
}

impl ParsedTraceQuerySession {
    pub fn from_parsed_traces(parsed_traces: Vec<ParsedTrace>) -> TraceResult<Self> {
        let ctx = SessionContext::new();
        register_parsed_traces(&ctx, &parsed_traces)?;
        Ok(Self { ctx })
    }

    pub fn from_parsed_trace_sources(sources: Vec<ParsedTraceSource>) -> TraceResult<Self> {
        let ctx = SessionContext::new();
        register_parsed_trace_sources(&ctx, sources)?;
        Ok(Self { ctx })
    }

    pub async fn query(&self, request: QueryRequest) -> TraceResult<QueryResult> {
        let dataframe = self
            .ctx
            .sql(&request.sql)
            .await
            .map_err(|err| TraceEngineError::UnsupportedSql(err.to_string()))?;
        let batches = dataframe
            .collect()
            .await
            .map_err(|err| TraceEngineError::Engine(format!("query execution failed: {err}")))?;
        batches_to_query_result(&batches, request.max_inline_rows)
    }
}
