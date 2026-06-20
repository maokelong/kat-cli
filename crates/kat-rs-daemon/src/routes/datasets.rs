use std::time::Instant;

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};

use crate::{
    api::{
        CreateDatasetRequest, DataEnvelope, DatasetQueryMeta, DatasetQueryRequest, DatasetResponse,
        QueryResponse,
    },
    error::{ApiError, ErrorEnvelope},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/datasets", post(create_dataset))
        .route("/v1/datasets/queries", post(query_dataset))
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
