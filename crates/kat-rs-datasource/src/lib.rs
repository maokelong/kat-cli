#![forbid(unsafe_code)]

//! datasource 门面、能力声明和 DataFusion 查询入口。

use std::time::Instant;

use anyhow::Result;

mod query;
mod result;

pub use query::DataFusionQuery;
pub use result::{QueryCell, QueryMetrics, QueryRequest, QueryResponse, QueryRow};

/// datasource 当前暴露给 CLI 壳的能力描述。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatasourceCapability {
    pub name: &'static str,
    pub description: &'static str,
}

/// datasource 已注册的能力列表。
pub const CAPABILITIES: &[DatasourceCapability] = &[DatasourceCapability {
    name: "trace-datasource",
    description: "Trace datasource library boundary",
}];

/// 返回 datasource 当前能力列表。
pub fn capabilities() -> &'static [DatasourceCapability] {
    CAPABILITIES
}

/// 检查 datasource workspace 内既有 crate 边界是否可用。
pub fn crate_boundaries_ready() -> bool {
    trace_parser::parser_shell_ready()
        && trace_query::query_shell_ready()
        && !trace_model::CRATE_ROLE.is_empty()
}

/// datasource 门面，负责 parse htrace 并委托 DataFusionQuery 执行 SQL。
#[derive(Debug, Default, Clone, Copy)]
pub struct TraceDatasource;

impl TraceDatasource {
    /// 创建 datasource 门面。
    pub fn new() -> Self {
        Self
    }

    /// 解析 htrace 文件，注册数据集，并执行 SQL。
    pub async fn query(&self, request: QueryRequest) -> Result<QueryResponse> {
        let total_start = Instant::now();
        let parse_start = Instant::now();
        let dataset = trace_htrace::parse(&request.trace_path)?;
        let parse_ms = elapsed_ms(parse_start);

        let query = DataFusionQuery::new(dataset)?;
        let mut response = query.query(&request.sql).await?;
        response.metrics.parse_ms = parse_ms;
        response.metrics.total_ms = elapsed_ms(total_start);

        Ok(response)
    }
}

/// 将 Instant 起点转换为经过的毫秒数。
fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::{capabilities, crate_boundaries_ready};

    #[test]
    fn exposes_datasource_boundary() {
        assert_eq!(capabilities()[0].name, "trace-datasource");
        assert!(crate_boundaries_ready());
    }
}
