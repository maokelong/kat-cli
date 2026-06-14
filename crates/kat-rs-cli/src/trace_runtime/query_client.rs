use anyhow::Result;
use serde_json::Value;

pub trait QueryClient {
    fn create_view(&mut self, name: &str, sql: &str) -> Result<()>;
    fn query_window(&mut self, request: QueryWindowRequest) -> Result<Vec<Value>>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryWindowMode {
    Window,
    Full,
    Metadata,
}

#[derive(Clone, Debug)]
pub struct QueryWindowRequest {
    pub target: String,
    pub mode: QueryWindowMode,
    pub time_column: Option<String>,
    pub duration_column: Option<String>,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub filters: Vec<(String, Value)>,
    pub limit: Option<u32>,
}
