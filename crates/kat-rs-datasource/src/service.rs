use crate::{
    DatasetHandle, DatasetInput, DatasetInspection, DatasetSummary, DatasourceQueryRequest,
    DatasourceResult, QueryEnvelope, TraceDatasource,
};

pub struct DatasourceService<D> {
    datasource: D,
}

impl<D> DatasourceService<D> {
    pub fn new(datasource: D) -> Self {
        Self { datasource }
    }
}

impl<D> DatasourceService<D>
where
    D: TraceDatasource,
{
    pub async fn open_dataset(&self, input: DatasetInput) -> DatasourceResult<DatasetHandle> {
        self.datasource.open_dataset(input).await
    }

    pub async fn list_datasets(&self) -> DatasourceResult<Vec<DatasetSummary>> {
        self.datasource.list_datasets().await
    }

    pub async fn close_dataset(&self, handle: &DatasetHandle) -> DatasourceResult<()> {
        self.datasource.close_dataset(handle).await
    }

    pub async fn inspect(&self, handle: &DatasetHandle) -> DatasourceResult<DatasetInspection> {
        self.datasource.inspect(handle).await
    }

    pub async fn query(
        &self,
        handle: &DatasetHandle,
        request: DatasourceQueryRequest,
    ) -> DatasourceResult<QueryEnvelope> {
        self.datasource.query(handle, request).await
    }
}
