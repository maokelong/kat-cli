use std::time::Instant;

use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;

use crate::{
    api::{
        CreateDatasetRequest, DataEnvelope, DatasetDto, DatasetInspectResponse, DatasetLocation,
        DatasetQueryMeta, DatasetQueryRequest, DatasetResponse, PaginatedEnvelope, Pagination,
        QueryResponse,
    },
    error::{ApiError, ErrorEnvelope},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/datasets", post(create_dataset).get(list_datasets))
        .route(
            "/v1/datasets/{dataset_name}",
            get(inspect_dataset).delete(delete_dataset),
        )
        .route("/v1/datasets/queries", post(query_dataset))
}

#[utoipa::path(
    get,
    path = "/v1/datasets",
    params(
        ("directory" = Option<String>, Query, description = "Absolute dataset root directory"),
        ("limit" = Option<usize>, Query, description = "Maximum number of datasets to return"),
        ("offset" = Option<usize>, Query, description = "Number of datasets to skip")
    ),
    responses(
        (status = 200, description = "Dataset list", body = PaginatedEnvelope<DatasetDto>),
        (status = 400, description = "Query parameters are malformed", body = ErrorEnvelope),
        (status = 422, description = "Dataset directory failed validation", body = ErrorEnvelope)
    )
)]
pub(crate) async fn list_datasets(
    State(state): State<AppState>,
    query: Result<Query<ListDatasetsQuery>, QueryRejection>,
) -> Result<Json<PaginatedEnvelope<DatasetDto>>, ApiError> {
    let Query(query) = query.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let list = state.dataset_service.list(
        query.directory,
        query.limit.unwrap_or(100),
        query.offset.unwrap_or(0),
    )?;

    Ok(Json(PaginatedEnvelope {
        data: list.data,
        pagination: Pagination {
            limit: list.limit,
            offset: list.offset,
            total_items: list.total_items,
        },
    }))
}

#[utoipa::path(
    post,
    path = "/v1/datasets",
    request_body = CreateDatasetRequest,
    responses(
        (status = 201, description = "Dataset was materialized", body = DataEnvelope<DatasetResponse>),
        (status = 400, description = "Request body is malformed", body = ErrorEnvelope),
        (status = 409, description = "Dataset target already exists", body = ErrorEnvelope),
        (status = 422, description = "Dataset materialization failed validation", body = ErrorEnvelope),
        (status = 500, description = "Internal server error", body = ErrorEnvelope)
    )
)]
pub(crate) async fn create_dataset(
    State(state): State<AppState>,
    request: Result<Json<CreateDatasetRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) =
        request.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let dataset = state.dataset_service.create(request).await?;

    Ok((
        StatusCode::CREATED,
        Json(DataEnvelope {
            data: DatasetResponse { dataset },
        }),
    )
        .into_response())
}

#[utoipa::path(
    get,
    path = "/v1/datasets/{datasetName}",
    params(
        ("datasetName" = String, Path, description = "Dataset name"),
        ("directory" = Option<String>, Query, description = "Absolute dataset root directory")
    ),
    responses(
        (status = 200, description = "Dataset metadata", body = DataEnvelope<DatasetInspectResponse>),
        (status = 400, description = "Query parameters are malformed", body = ErrorEnvelope),
        (status = 404, description = "Dataset not found", body = ErrorEnvelope),
        (status = 422, description = "Dataset validation failed", body = ErrorEnvelope)
    )
)]
pub(crate) async fn inspect_dataset(
    State(state): State<AppState>,
    Path(dataset_name): Path<String>,
    query: Result<Query<DatasetRootQuery>, QueryRejection>,
) -> Result<Json<DataEnvelope<DatasetInspectResponse>>, ApiError> {
    let Query(query) = query.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let data = state.dataset_service.inspect(DatasetLocation {
        name: dataset_name,
        directory: query.directory,
    })?;

    Ok(Json(DataEnvelope { data }))
}

#[utoipa::path(
    delete,
    path = "/v1/datasets/{datasetName}",
    params(
        ("datasetName" = String, Path, description = "Dataset name"),
        ("directory" = Option<String>, Query, description = "Absolute dataset root directory")
    ),
    responses(
        (status = 204, description = "Dataset deleted"),
        (status = 400, description = "Query parameters are malformed", body = ErrorEnvelope),
        (status = 404, description = "Dataset not found", body = ErrorEnvelope),
        (status = 422, description = "Dataset validation failed", body = ErrorEnvelope)
    )
)]
pub(crate) async fn delete_dataset(
    State(state): State<AppState>,
    Path(dataset_name): Path<String>,
    query: Result<Query<DatasetRootQuery>, QueryRejection>,
) -> Result<StatusCode, ApiError> {
    let Query(query) = query.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    state.dataset_service.delete(DatasetLocation {
        name: dataset_name,
        directory: query.directory,
    })?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/v1/datasets/queries",
    request_body = DatasetQueryRequest,
    responses(
        (status = 200, description = "Dataset query result", body = QueryResponse<DatasetQueryMeta>),
        (status = 400, description = "Request body is malformed", body = ErrorEnvelope),
        (status = 404, description = "Dataset not found", body = ErrorEnvelope),
        (status = 422, description = "Dataset validation or query failed", body = ErrorEnvelope),
        (status = 500, description = "Internal server error", body = ErrorEnvelope)
    )
)]
pub(crate) async fn query_dataset(
    State(state): State<AppState>,
    request: Result<Json<DatasetQueryRequest>, JsonRejection>,
) -> Result<Json<QueryResponse<DatasetQueryMeta>>, ApiError> {
    let Json(request) =
        request.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let started_at = Instant::now();
    let (dataset, rows) = state.dataset_service.query(request).await?;
    let meta = DatasetQueryMeta {
        elapsed_ms: started_at.elapsed().as_millis(),
        dataset,
    };

    Ok(Json(QueryResponse::new(meta, rows)))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListDatasetsQuery {
    directory: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DatasetRootQuery {
    directory: Option<String>,
}
