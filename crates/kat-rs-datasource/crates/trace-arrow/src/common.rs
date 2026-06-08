//! build 阶段和运行时共享的 Arrow 数据结构。

use std::collections::BTreeSet;

use anyhow::{ensure, Result};
use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;

/// 一张 SQL 表的数据，包含同一 schema 下的多个 batch。
#[derive(Debug, Clone)]
pub struct ArrowTable {
    pub name: String,
    pub schema: SchemaRef,
    pub batches: Vec<RecordBatch>,
}

impl ArrowTable {
    /// 创建一张 Arrow 表，并校验 batch schema 与表级 schema 一致。
    pub fn new(
        name: impl Into<String>,
        schema: SchemaRef,
        batches: Vec<RecordBatch>,
    ) -> Result<Self> {
        let name = name.into();
        ensure!(!name.is_empty(), "arrow table name must not be empty");
        ensure!(!batches.is_empty(), "arrow table batches must not be empty");

        for batch in &batches {
            ensure!(
                batch.schema().as_ref() == schema.as_ref(),
                "arrow table `{name}` contains a batch with a different schema"
            );
            ensure!(
                batch.num_rows() > 0,
                "arrow table `{name}` contains an empty batch"
            );
        }

        Ok(Self {
            name,
            schema,
            batches,
        })
    }
}

/// 一次 trace 解析后的多表集合封装。
#[derive(Debug, Clone, Default)]
pub struct TraceDataset {
    tables: Vec<ArrowTable>,
}

impl TraceDataset {
    /// 从表迭代器创建数据集，并校验表名不重复。
    pub fn from_tables(tables: impl IntoIterator<Item = ArrowTable>) -> Result<Self> {
        let tables = tables.into_iter().collect::<Vec<_>>();
        let mut names = BTreeSet::new();

        for table in &tables {
            ensure!(
                names.insert(table.name.as_str()),
                "duplicate arrow table name: {}",
                table.name
            );
        }

        Ok(Self { tables })
    }

    /// 返回数据集中所有表的只读迭代器。
    pub fn tables(&self) -> impl Iterator<Item = &ArrowTable> + '_ {
        self.tables.iter()
    }
}
