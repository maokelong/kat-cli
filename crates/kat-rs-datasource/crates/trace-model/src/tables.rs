use arrow_array::RecordBatch;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct TraceTables {
    pub trace_bounds: RecordBatch,
}

impl TraceTables {
    /// 返回按表名索引的已解析 trace 批数据。
    pub fn batches(&self) -> BTreeMap<&'static str, RecordBatch> {
        BTreeMap::from([("trace_bounds", self.trace_bounds.clone())])
    }
}

#[derive(Debug, Clone)]
pub struct ParsedTrace {
    pub trace_id: String,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub clock_domain: String,
    pub tables: TraceTables,
}

impl ParsedTrace {
    /// 返回当前 parsed trace 内部的所有表数据。
    pub fn batches(&self) -> BTreeMap<&'static str, RecordBatch> {
        self.tables.batches()
    }
}
