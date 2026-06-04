use arrow_array::RecordBatch;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct TraceTables {
    batches: BTreeMap<&'static str, RecordBatch>,
}

impl TraceTables {
    /// 创建一个不包含任何表数据的 trace 表集合。
    pub fn new() -> Self {
        Self {
            batches: BTreeMap::new(),
        }
    }

    /// 插入非空 RecordBatch，空批数据不会进入结果集合。
    pub fn insert(&mut self, table_name: &'static str, batch: RecordBatch) {
        if batch.num_rows() > 0 {
            self.batches.insert(table_name, batch);
        }
    }

    /// 返回按表名索引的已解析 trace 批数据。
    pub fn batches(&self) -> BTreeMap<&'static str, RecordBatch> {
        self.batches.clone()
    }

    /// 按表名读取已经生成的 RecordBatch。
    pub fn get(&self, table_name: &str) -> Option<&RecordBatch> {
        self.batches.get(table_name)
    }
}

impl Default for TraceTables {
    /// 创建默认的空 trace 表集合。
    fn default() -> Self {
        Self::new()
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
