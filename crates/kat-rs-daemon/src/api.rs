use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DataEnvelope<T> {
    pub data: T,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}
