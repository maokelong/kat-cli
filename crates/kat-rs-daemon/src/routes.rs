use axum::{Router, routing::get};
use tower_http::trace::TraceLayer;

use crate::state::AppState;

mod datasources;
mod health;
mod queries;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health::health))
        .merge(datasources::routes())
        .merge(queries::routes())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
