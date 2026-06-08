//! datasource 查询输入、输出和指标类型。

use std::path::PathBuf;

/// 一次 datasource SQL 查询的输入。
#[derive(Debug, Clone)]
pub struct QueryRequest {
    pub trace_path: PathBuf,
    pub sql: String,
}

impl QueryRequest {
    /// 根据 trace 路径和 SQL 创建查询请求。
    pub fn new(trace_path: impl Into<PathBuf>, sql: impl Into<String>) -> Self {
        Self {
            trace_path: trace_path.into(),
            sql: sql.into(),
        }
    }
}

/// datasource SQL 查询响应。
#[derive(Debug, Clone)]
pub struct QueryResponse {
    pub row_count: usize,
    pub rows: Vec<QueryRow>,
    pub metrics: QueryMetrics,
}

/// 查询结果中的一行。
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct QueryRow {
    pub cells: Vec<QueryCell>,
}

/// 查询结果中的一个单元格。
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct QueryCell {
    pub name: String,
    pub value: String,
}

/// 查询耗时指标，单位毫秒。
#[derive(Debug, Default, Clone, Copy)]
pub struct QueryMetrics {
    pub parse_ms: f64,
    pub register_ms: f64,
    pub sql_ms: f64,
    pub format_ms: f64,
    pub total_ms: f64,
}
