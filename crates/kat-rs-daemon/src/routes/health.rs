use axum::Json;

use crate::api::{DataEnvelope, HealthResponse};

#[utoipa::path(
    get,
    path = "/v1/health",
    responses(
        (status = 200, description = "Server is healthy", body = DataEnvelope<HealthResponse>)
    )
)]
pub(crate) async fn health() -> Json<DataEnvelope<HealthResponse>> {
    Json(DataEnvelope {
        data: HealthResponse { status: "ok" },
    })
}
