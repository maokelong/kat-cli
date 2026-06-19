use axum::{Json, Router, routing::get};
use serde_json::Value;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

mod datasources;
mod health;
mod queries;
mod server;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/openapi.json", get(openapi))
        .route("/v1/health", get(health::health))
        .merge(datasources::routes())
        .merge(queries::routes())
        .merge(server::routes())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn openapi() -> Json<Value> {
    Json(crate::openapi::openapi_document())
}
