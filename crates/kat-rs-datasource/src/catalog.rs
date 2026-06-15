// 表目录只描述 sink 产出的查询表，不承载解析阶段的中间协议。

use arrow_array::RecordBatch;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TableCategory {
    Raw,
    DirectEvent,
}

pub(crate) struct TraceTable {
    pub(crate) name: &'static str,
    pub(crate) category: TableCategory,
    pub(crate) batches: Vec<RecordBatch>,
}

impl TraceTable {
    pub(crate) fn new(
        name: &'static str,
        category: TableCategory,
        batches: Vec<RecordBatch>,
    ) -> Self {
        Self {
            name,
            category,
            batches,
        }
    }
}

pub(crate) struct TraceDataset {
    pub(crate) tables: Vec<TraceTable>,
}

impl TraceDataset {
    pub(crate) fn new(tables: Vec<TraceTable>) -> Self {
        Self { tables }
    }
}
