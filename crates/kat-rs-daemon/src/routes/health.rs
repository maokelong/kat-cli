use axum::Json;

use crate::api::{DataEnvelope, HealthResponse};

pub async fn health() -> Json<DataEnvelope<HealthResponse>> {
    Json(DataEnvelope {
        data: HealthResponse { status: "ok" },
    })
}
