use axum::{Router, extract::Path, routing::get};
use tower_http::trace::TraceLayer;

use crate::error::ApiError;
use crate::state::AppState;

mod health;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health::health))
        .route("/v1/datasources/{datasource_id}", get(get_datasource))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn get_datasource(Path(datasource_id): Path<String>) -> Result<(), ApiError> {
    Err(ApiError::datasource_not_found(datasource_id))
}
