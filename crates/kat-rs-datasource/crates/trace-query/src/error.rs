use thiserror::Error;

pub type TraceResult<T> = Result<T, TraceEngineError>;

#[derive(Debug, Error)]
pub enum TraceEngineError {
    #[error("trace parse error: {0}")]
    Parse(String),
    #[error("unsupported schema: {0}")]
    UnsupportedSchema(String),
    #[error("unsupported sql: {0}")]
    UnsupportedSql(String),
    #[error("invalid params: {0}")]
    InvalidParams(String),
    #[error("query timeout")]
    Timeout,
    #[error("result too large: {0}")]
    ResultTooLarge(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("engine error: {0}")]
    Engine(String),
}

impl From<trace_parser::TraceEngineError> for TraceEngineError {
    fn from(value: trace_parser::TraceEngineError) -> Self {
        match value {
            trace_parser::TraceEngineError::Parse(message) => Self::Parse(message),
            trace_parser::TraceEngineError::UnsupportedSchema(message) => {
                Self::UnsupportedSchema(message)
            }
            trace_parser::TraceEngineError::UnsupportedSql(message) => {
                Self::UnsupportedSql(message)
            }
            trace_parser::TraceEngineError::InvalidParams(message) => Self::InvalidParams(message),
            trace_parser::TraceEngineError::Timeout => Self::Timeout,
            trace_parser::TraceEngineError::ResultTooLarge(message) => {
                Self::ResultTooLarge(message)
            }
            trace_parser::TraceEngineError::Io(error) => Self::Io(error),
            trace_parser::TraceEngineError::Engine(message) => Self::Engine(message),
        }
    }
}
