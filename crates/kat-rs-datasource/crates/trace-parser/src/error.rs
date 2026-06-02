use thiserror::Error;

pub type TraceResult<T> = Result<T, TraceEngineError>;
pub type ParseResult<T> = TraceResult<T>;

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
