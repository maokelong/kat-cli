use thiserror::Error;
use trace_query::TraceEngineError;

pub type DatasourceResult<T> = Result<T, DatasourceError>;

#[derive(Debug, Error)]
pub enum DatasourceError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("unsupported schema: {0}")]
    UnsupportedSchema(String),
    #[error("unsupported sql: {0}")]
    UnsupportedSql(String),
    #[error("query timeout")]
    Timeout,
    #[error("result too large: {0}")]
    ResultTooLarge(String),
    #[error("engine error: {0}")]
    Engine(String),
}

impl From<TraceEngineError> for DatasourceError {
    fn from(value: TraceEngineError) -> Self {
        match value {
            TraceEngineError::UnsupportedSchema(message) => Self::UnsupportedSchema(message),
            TraceEngineError::UnsupportedSql(message) => Self::UnsupportedSql(message),
            TraceEngineError::InvalidParams(message) => Self::InvalidInput(message),
            TraceEngineError::Timeout => Self::Timeout,
            TraceEngineError::ResultTooLarge(message) => Self::ResultTooLarge(message),
            TraceEngineError::Parse(message) => Self::Engine(format!("parse error: {message}")),
            TraceEngineError::Io(error) => Self::Engine(error.to_string()),
            TraceEngineError::Engine(message) => Self::Engine(message),
        }
    }
}

impl From<trace_parser::TraceEngineError> for DatasourceError {
    fn from(value: trace_parser::TraceEngineError) -> Self {
        match value {
            trace_parser::TraceEngineError::UnsupportedSchema(message) => {
                Self::UnsupportedSchema(message)
            }
            trace_parser::TraceEngineError::UnsupportedSql(message) => {
                Self::UnsupportedSql(message)
            }
            trace_parser::TraceEngineError::InvalidParams(message) => Self::InvalidInput(message),
            trace_parser::TraceEngineError::Timeout => Self::Timeout,
            trace_parser::TraceEngineError::ResultTooLarge(message) => {
                Self::ResultTooLarge(message)
            }
            trace_parser::TraceEngineError::Parse(message) => {
                Self::Engine(format!("parse error: {message}"))
            }
            trace_parser::TraceEngineError::Io(error) => Self::Engine(error.to_string()),
            trace_parser::TraceEngineError::Engine(message) => Self::Engine(message),
        }
    }
}

impl From<std::io::Error> for DatasourceError {
    fn from(value: std::io::Error) -> Self {
        Self::Engine(value.to_string())
    }
}

impl From<serde_json::Error> for DatasourceError {
    fn from(value: serde_json::Error) -> Self {
        Self::Engine(value.to_string())
    }
}
