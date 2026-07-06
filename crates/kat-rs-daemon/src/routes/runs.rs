use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    routing::{get, post},
};

use crate::{
    api::{
        CreateRunRequest, DataEnvelope, RunBriefResponse, RunDetailDto, RunEvidenceResponse,
        RunSummaryDto,
    },
    error::{ApiError, ErrorEnvelope},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/runs", post(create_run))
        .route("/v1/runs/{run_id}", get(get_run))
        .route("/v1/runs/{run_id}/evidence", get(get_run_evidence))
        .route("/v1/runs/{run_id}/brief", get(get_run_brief))
}

#[utoipa::path(
    post,
    path = "/v1/runs",
    request_body = CreateRunRequest,
    responses(
        (status = 200, description = "Run was executed", body = DataEnvelope<RunSummaryDto>),
        (status = 400, description = "Request body is malformed", body = ErrorEnvelope),
        (status = 404, description = "Dataset not found", body = ErrorEnvelope),
        (status = 422, description = "Dataset validation failed", body = ErrorEnvelope)
    )
)]
pub(crate) async fn create_run(
    State(state): State<AppState>,
    request: Result<Json<CreateRunRequest>, JsonRejection>,
) -> Result<Json<DataEnvelope<RunSummaryDto>>, ApiError> {
    let Json(request) =
        request.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let dataset = state
        .dataset_service
        .resolve_existing(request.dataset.clone())?;
    let run = state.run_service.create(request, dataset).await?;

    Ok(Json(DataEnvelope {
        data: run.to_summary_dto(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/runs/{runId}",
    params(
        ("runId" = String, Path, description = "Run id")
    ),
    responses(
        (status = 200, description = "Run detail", body = DataEnvelope<RunDetailDto>),
        (status = 404, description = "Run not found", body = ErrorEnvelope)
    )
)]
pub(crate) async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<DataEnvelope<RunDetailDto>>, ApiError> {
    let run = state.run_service.get(&run_id).await?;

    Ok(Json(DataEnvelope {
        data: run.to_detail_dto(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/runs/{runId}/evidence",
    params(
        ("runId" = String, Path, description = "Run id")
    ),
    responses(
        (status = 200, description = "Run evidence", body = DataEnvelope<RunEvidenceResponse>),
        (status = 404, description = "Run not found", body = ErrorEnvelope)
    )
)]
pub(crate) async fn get_run_evidence(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<DataEnvelope<RunEvidenceResponse>>, ApiError> {
    let run = state.run_service.get(&run_id).await?;

    Ok(Json(DataEnvelope {
        data: run.to_evidence_response(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/runs/{runId}/brief",
    params(
        ("runId" = String, Path, description = "Run id")
    ),
    responses(
        (status = 200, description = "Run brief", body = DataEnvelope<RunBriefResponse>),
        (status = 404, description = "Run not found", body = ErrorEnvelope)
    )
)]
pub(crate) async fn get_run_brief(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<DataEnvelope<RunBriefResponse>>, ApiError> {
    let run = state.run_service.get(&run_id).await?;

    Ok(Json(DataEnvelope {
        data: run.to_brief_response(),
    }))
}
