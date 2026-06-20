use std::time::Instant;

use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    routing::post,
};

use crate::{
    api::{DatasourceQueryMeta, QueryRequest, QueryResponse},
    error::{ApiError, ErrorEnvelope},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/datasources/{datasource_id}/queries", post(query))
}

#[utoipa::path(
    post,
    path = "/v1/datasources/{datasourceId}/queries",
    request_body = QueryRequest,
    params(
        ("datasourceId" = String, Path, description = "Datasource id")
    ),
    responses(
        (status = 200, description = "Query result", body = QueryResponse<DatasourceQueryMeta>),
        (status = 400, description = "Request body is malformed", body = ErrorEnvelope),
        (status = 404, description = "Datasource not found", body = ErrorEnvelope),
        (status = 422, description = "Query failed", body = ErrorEnvelope)
    )
)]
pub(crate) async fn query(
    State(state): State<AppState>,
    Path(datasource_id): Path<String>,
    request: Result<Json<QueryRequest>, JsonRejection>,
) -> Result<Json<QueryResponse<DatasourceQueryMeta>>, ApiError> {
    let Json(request) =
        request.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let started_at = Instant::now();
    let rows = state
        .datasource_service
        .query(&datasource_id, request)
        .await?;
    let meta = DatasourceQueryMeta {
        elapsed_ms: started_at.elapsed().as_millis(),
        datasource_id,
    };

    Ok(Json(QueryResponse::new(meta, rows)))
}
