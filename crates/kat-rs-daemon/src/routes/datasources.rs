use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection, rejection::QueryRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;

use crate::{
    api::{CreateDatasourceRequest, DataEnvelope, DatasourceDto, PaginatedEnvelope, Pagination},
    error::ApiError,
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/datasources",
            post(create_datasource).get(list_datasources),
        )
        .route(
            "/v1/datasources/{datasource_id}",
            get(get_datasource).delete(delete_datasource),
        )
}

async fn create_datasource(
    State(state): State<AppState>,
    request: Result<Json<CreateDatasourceRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) =
        request.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let (datasource, created) = state.datasource_service.create(request).await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };

    Ok((status, Json(DataEnvelope { data: datasource })).into_response())
}

async fn list_datasources(
    State(state): State<AppState>,
    query: Result<Query<ListDatasourcesQuery>, QueryRejection>,
) -> Result<Json<PaginatedEnvelope<DatasourceDto>>, ApiError> {
    let Query(query) = query.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);
    let list = state.datasource_service.list(limit, offset).await;

    Ok(Json(PaginatedEnvelope {
        data: list.data,
        pagination: Pagination {
            limit: list.limit,
            offset: list.offset,
            total_items: list.total_items,
        },
    }))
}

async fn get_datasource(
    State(state): State<AppState>,
    Path(datasource_id): Path<String>,
) -> Result<Json<DataEnvelope<DatasourceDto>>, ApiError> {
    let datasource = state.datasource_service.get(&datasource_id).await?;

    Ok(Json(DataEnvelope { data: datasource }))
}

async fn delete_datasource(
    State(state): State<AppState>,
    Path(datasource_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.datasource_service.delete(&datasource_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct ListDatasourcesQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}
