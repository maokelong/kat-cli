use std::time::Instant;

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};

use crate::{
    api::{DataEnvelopeWithMeta, QueryMeta, QueryRequest, QueryResponse},
    error::ApiError,
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/datasources/{datasource_id}/queries", post(query))
}

async fn query(
    State(state): State<AppState>,
    Path(datasource_id): Path<String>,
    Json(request): Json<QueryRequest>,
) -> Result<Json<DataEnvelopeWithMeta<QueryResponse, QueryMeta>>, ApiError> {
    let started_at = Instant::now();
    let data = state
        .datasource_service
        .query(&datasource_id, request)
        .await?;
    let meta = QueryMeta {
        datasource_id,
        elapsed_ms: started_at.elapsed().as_millis(),
    };

    Ok(Json(DataEnvelopeWithMeta { data, meta }))
}
