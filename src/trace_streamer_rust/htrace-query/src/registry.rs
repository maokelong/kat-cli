use crate::json::batches_to_query_result;
use datafusion::datasource::MemTable;
use datafusion::prelude::SessionContext;
use htrace_core::{QueryRequest, QueryResult, TraceEngineError, TraceResult};
use htrace_model::ParsedTrace;
use std::sync::Arc;

pub fn register_parsed_trace(ctx: &SessionContext, parsed: &ParsedTrace) -> TraceResult<()> {
    for (name, batch) in parsed.tables.batches() {
        let provider = MemTable::try_new(batch.schema(), vec![vec![batch]]).map_err(|err| {
            TraceEngineError::Engine(format!("failed to build MemTable {name}: {err}"))
        })?;
        ctx.register_table(name, Arc::new(provider))
            .map_err(|err| {
                TraceEngineError::Engine(format!("failed to register table {name}: {err}"))
            })?;
    }
    Ok(())
}

pub async fn query_parsed_trace(
    parsed: &ParsedTrace,
    request: QueryRequest,
) -> TraceResult<QueryResult> {
    let ctx = SessionContext::new();
    register_parsed_trace(&ctx, parsed)?;
    let dataframe = ctx
        .sql(&request.sql)
        .await
        .map_err(|err| TraceEngineError::UnsupportedSql(err.to_string()))?;
    let batches = dataframe
        .collect()
        .await
        .map_err(|err| TraceEngineError::Engine(format!("query execution failed: {err}")))?;
    batches_to_query_result(&batches, request.max_inline_rows)
}
