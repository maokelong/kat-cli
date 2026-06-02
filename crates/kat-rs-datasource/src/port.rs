use crate::{
    DatasetHandle, DatasetInput, DatasetInspection, DatasetSummary, DatasourceQueryRequest,
    DatasourceResult, QueryEnvelope,
};
use async_trait::async_trait;

#[async_trait]
pub trait TraceDatasource: Send + Sync {
    async fn open_dataset(&self, input: DatasetInput) -> DatasourceResult<DatasetHandle>;

    async fn list_datasets(&self) -> DatasourceResult<Vec<DatasetSummary>>;

    async fn close_dataset(&self, handle: &DatasetHandle) -> DatasourceResult<()>;

    async fn inspect(&self, handle: &DatasetHandle) -> DatasourceResult<DatasetInspection>;

    async fn query(
        &self,
        handle: &DatasetHandle,
        request: DatasourceQueryRequest,
    ) -> DatasourceResult<QueryEnvelope>;
}
