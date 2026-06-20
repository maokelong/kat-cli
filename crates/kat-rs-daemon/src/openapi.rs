use serde_json::Value;
use utoipa::OpenApi;

use crate::{
    api::{
        CreateDatasetRequest, CreateDatasourceRequest, DatasetDto, DatasetLocation,
        DatasetQueryMeta, DatasetQueryRequest, DatasetResponse, DatasetSourceInput, DatasourceDto,
        DatasourceQueryMeta, DatasourceSource, HealthResponse, InputFileDto, InputRole, Pagination,
        QueryRequest, QueryResponse, ShutdownResponse,
    },
    error::{ErrorBody, ErrorCode, ErrorEnvelope},
};

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::routes::health::health,
        crate::routes::datasets::create_dataset,
        crate::routes::datasets::query_dataset,
        crate::routes::datasources::create_datasource,
        crate::routes::datasources::list_datasources,
        crate::routes::datasources::get_datasource,
        crate::routes::datasources::delete_datasource,
        crate::routes::queries::query,
        crate::routes::server::shutdown
    ),
    components(schemas(
        CreateDatasourceRequest,
        CreateDatasetRequest,
        DatasetDto,
        DatasetLocation,
        DatasetQueryMeta,
        DatasetQueryRequest,
        DatasetResponse,
        DatasetSourceInput,
        DatasourceDto,
        DatasourceQueryMeta,
        DatasourceSource,
        ErrorBody,
        ErrorCode,
        ErrorEnvelope,
        HealthResponse,
        InputFileDto,
        InputRole,
        Pagination,
        QueryRequest,
        QueryResponse<DatasetQueryMeta>,
        QueryResponse<DatasourceQueryMeta>,
        ShutdownResponse
    ))
)]
struct ApiDoc;

pub fn openapi_document() -> Value {
    let mut document = ApiDoc::openapi();
    document.info.title = "kat-rs local API".to_owned();

    serde_json::to_value(document).expect("generated OpenAPI document serializes")
}
