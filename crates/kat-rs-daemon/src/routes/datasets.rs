use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};

use crate::{
    api::{CreateDatasetRequest, DataEnvelope, DatasetResponse},
    error::{ApiError, ErrorEnvelope},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/datasets", post(create_dataset))
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
