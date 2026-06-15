use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    BadRequest,
    DatasourceNotFound,
    Internal,
    QueryFailed,
    ValidationFailed,
}

#[derive(Clone, Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<Value>,
}

impl ApiError {
    pub fn datasource_not_found(datasource_id: impl Into<String>) -> Self {
        let datasource_id = datasource_id.into();

        Self {
            status: StatusCode::NOT_FOUND,
            code: ErrorCode::DatasourceNotFound,
            message: "datasource not found".to_owned(),
            details: Some(json!({ "datasourceId": datasource_id })),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: ErrorCode::ValidationFailed,
            message: message.into(),
            details: None,
        }
    }

    pub fn query_failed(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: ErrorCode::QueryFailed,
            message: message.into(),
            details: None,
        }
    }

    pub fn internal(_message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: ErrorCode::Internal,
            message: "internal server error".to_string(),
            details: None,
        }
    }
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: ErrorCode,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorEnvelope {
            error: ErrorBody {
                code: self.code,
                message: self.message,
                details: self.details,
            },
        };

        (self.status, Json(body)).into_response()
    }
}
