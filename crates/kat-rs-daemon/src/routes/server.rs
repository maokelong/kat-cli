use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::delete,
};

use crate::{
    api::{DataEnvelope, ShutdownResponse},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/server", delete(shutdown))
}

#[utoipa::path(
    delete,
    path = "/v1/server",
    responses(
        (status = 202, description = "Shutdown accepted", body = DataEnvelope<ShutdownResponse>)
    )
)]
pub(crate) async fn shutdown(State(state): State<AppState>) -> Response {
    state.shutdown.notify_waiters();

    (
        StatusCode::ACCEPTED,
        Json(DataEnvelope {
            data: ShutdownResponse {
                state: "SHUTTING_DOWN",
            },
        }),
    )
        .into_response()
}
