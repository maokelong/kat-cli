use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Value, json};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    BadRequest,
    Conflict,
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
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: ErrorCode::BadRequest,
            message: message.into(),
            details: None,
        }
    }

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

    pub fn conflict(message: impl Into<String>, details: Option<Value>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: ErrorCode::Conflict,
            message: message.into(),
            details,
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

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
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
