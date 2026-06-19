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
    error::{ApiError, ErrorEnvelope},
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

#[utoipa::path(
    post,
    path = "/v1/datasources",
    request_body = CreateDatasourceRequest,
    responses(
        (status = 200, description = "Existing datasource was reused", body = DataEnvelope<DatasourceDto>),
        (status = 201, description = "Datasource was created", body = DataEnvelope<DatasourceDto>),
        (status = 400, description = "Request body is malformed", body = ErrorEnvelope),
        (status = 422, description = "Datasource input failed validation", body = ErrorEnvelope),
        (status = 500, description = "Internal server error", body = ErrorEnvelope)
    )
)]
pub(crate) async fn create_datasource(
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

#[utoipa::path(
    get,
    path = "/v1/datasources",
    params(
        ("limit" = Option<usize>, Query, description = "Maximum number of datasources to return"),
        ("offset" = Option<usize>, Query, description = "Number of datasources to skip")
    ),
    responses(
        (status = 200, description = "Datasource list", body = PaginatedEnvelope<DatasourceDto>),
        (status = 400, description = "Query parameters are malformed", body = ErrorEnvelope)
    )
)]
pub(crate) async fn list_datasources(
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

#[utoipa::path(
    get,
    path = "/v1/datasources/{datasourceId}",
    params(
        ("datasourceId" = String, Path, description = "Datasource id")
    ),
    responses(
        (status = 200, description = "Datasource metadata", body = DataEnvelope<DatasourceDto>),
        (status = 404, description = "Datasource not found", body = ErrorEnvelope)
    )
)]
pub(crate) async fn get_datasource(
    State(state): State<AppState>,
    Path(datasource_id): Path<String>,
) -> Result<Json<DataEnvelope<DatasourceDto>>, ApiError> {
    let datasource = state.datasource_service.get(&datasource_id).await?;

    Ok(Json(DataEnvelope { data: datasource }))
}

#[utoipa::path(
    delete,
    path = "/v1/datasources/{datasourceId}",
    params(
        ("datasourceId" = String, Path, description = "Datasource id")
    ),
    responses(
        (status = 204, description = "Datasource deleted"),
        (status = 404, description = "Datasource not found", body = ErrorEnvelope)
    )
)]
pub(crate) async fn delete_datasource(
    State(state): State<AppState>,
    Path(datasource_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.datasource_service.delete(&datasource_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListDatasourcesQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}
