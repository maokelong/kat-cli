use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};

use crate::{
    api::{CreateRunRequest, DataEnvelope, RunDto, RunEvidenceResponse},
    error::{ApiError, ErrorEnvelope},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/runs", post(create_run))
        .route("/v1/runs/{run_id}", get(get_run))
        .route("/v1/runs/{run_id}/evidence", get(get_run_evidence))
}

#[utoipa::path(
    post,
    path = "/v1/runs",
    request_body = CreateRunRequest,
    responses(
        (status = 201, description = "Run executed synchronously", body = DataEnvelope<RunDto>),
        (status = 400, description = "Request body is malformed", body = ErrorEnvelope),
        (status = 404, description = "Dataset not found", body = ErrorEnvelope),
        (status = 422, description = "Run failed validation or execution", body = ErrorEnvelope)
    )
)]
pub(crate) async fn create_run(
    State(state): State<AppState>,
    request: Result<Json<CreateRunRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) =
        request.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let run = state
        .run_service
        .create(&state.dataset_service, request)
        .await?;

    Ok((StatusCode::CREATED, Json(DataEnvelope { data: run })).into_response())
}

#[utoipa::path(
    get,
    path = "/v1/runs/{runId}",
    params(("runId" = String, Path, description = "Run id")),
    responses(
        (status = 200, description = "Run metadata", body = DataEnvelope<RunDto>),
        (status = 404, description = "Run not found", body = ErrorEnvelope)
    )
)]
pub(crate) async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<DataEnvelope<RunDto>>, ApiError> {
    Ok(Json(DataEnvelope {
        data: state.run_service.get(&run_id)?,
    }))
}

#[utoipa::path(
    get,
    path = "/v1/runs/{runId}/evidence",
    params(("runId" = String, Path, description = "Run id")),
    responses(
        (status = 200, description = "Run evidence", body = DataEnvelope<RunEvidenceResponse>),
        (status = 404, description = "Run not found", body = ErrorEnvelope)
    )
)]
pub(crate) async fn get_run_evidence(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<DataEnvelope<RunEvidenceResponse>>, ApiError> {
    Ok(Json(DataEnvelope {
        data: state.run_service.evidence(&run_id)?,
    }))
}
