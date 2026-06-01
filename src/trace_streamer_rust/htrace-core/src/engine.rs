use crate::{
    OpenOptions, QueryRequest, QueryResult, TraceHandle, TraceInput, TraceInspection, TraceResult,
};

#[async_trait::async_trait]
pub trait TraceQueryEngine: Send + Sync {
    async fn open(&self, input: TraceInput, options: OpenOptions) -> TraceResult<TraceHandle>;
    async fn inspect(&self, handle: &TraceHandle) -> TraceResult<TraceInspection>;
    async fn query(&self, handle: &TraceHandle, request: QueryRequest) -> TraceResult<QueryResult>;
    async fn close(&self, handle: TraceHandle) -> TraceResult<()>;
}
