//! DataFusionQuery 查询实体，负责注册 TraceDataset 并执行 SQL。

use std::sync::Arc;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use arrow_array::{
    Array, BinaryArray, BooleanArray, Float32Array, Float64Array, Int32Array, Int64Array,
    RecordBatch, StringArray, UInt32Array, UInt64Array,
};
use arrow_schema::DataType;
use datafusion::datasource::MemTable;
use datafusion::prelude::SessionContext;
use trace_arrow::TraceDataset;

use crate::{QueryCell, QueryMetrics, QueryResponse, QueryRow};

/// 持有已注册 TraceDataset 的 DataFusion 查询实体。
pub struct DataFusionQuery {
    ctx: SessionContext,
    register_ms: f64,
}

impl DataFusionQuery {
    /// 注册数据集内全部表和 batch，创建查询实体。
    pub fn new(dataset: TraceDataset) -> Result<Self> {
        let register_start = Instant::now();
        let ctx = SessionContext::new();

        for table in dataset.tables() {
            let provider = MemTable::try_new(table.schema.clone(), vec![table.batches.clone()])
                .with_context(|| format!("failed to create MemTable for `{}`", table.name))?;
            ctx.register_table(&table.name, Arc::new(provider))
                .with_context(|| format!("failed to register table `{}`", table.name))?;
        }

        Ok(Self {
            ctx,
            register_ms: elapsed_ms(register_start),
        })
    }

    /// 对已注册数据执行 SQL，并格式化查询结果。
    pub async fn query(&self, sql: &str) -> Result<QueryResponse> {
        let total_start = Instant::now();
        let sql_start = Instant::now();
        let dataframe = self.ctx.sql(sql).await?;
        let batches = dataframe.collect().await?;
        let sql_ms = elapsed_ms(sql_start);

        let format_start = Instant::now();
        let (row_count, rows) = format_query_batches(&batches)?;
        let format_ms = elapsed_ms(format_start);

        Ok(QueryResponse {
            row_count,
            rows,
            metrics: QueryMetrics {
                parse_ms: 0.0,
                register_ms: self.register_ms,
                sql_ms,
                format_ms,
                total_ms: elapsed_ms(total_start),
            },
        })
    }
}

/// 将 DataFusion RecordBatch 转换为稳定行输出。
fn format_query_batches(batches: &[RecordBatch]) -> Result<(usize, Vec<QueryRow>)> {
    let row_count = batches.iter().map(RecordBatch::num_rows).sum::<usize>();
    let mut rows = Vec::with_capacity(row_count);

    for batch in batches {
        let schema = batch.schema();
        for row in 0..batch.num_rows() {
            let mut cells = Vec::with_capacity(batch.num_columns());
            for (column_index, field) in schema.fields().iter().enumerate() {
                let value = array_value_to_string(
                    batch.column(column_index).as_ref(),
                    field.data_type(),
                    row,
                )?;
                cells.push(QueryCell {
                    name: field.name().to_string(),
                    value,
                });
            }
            rows.push(QueryRow { cells });
        }
    }

    Ok((row_count, rows))
}

/// 将单个 Arrow 值转换为 CLI 友好的字符串。
fn array_value_to_string(array: &dyn Array, data_type: &DataType, row: usize) -> Result<String> {
    if array.is_null(row) {
        return Ok("null".to_string());
    }

    match data_type {
        DataType::Boolean => Ok(array
            .as_any()
            .downcast_ref::<BooleanArray>()
            .context("Boolean column should be BooleanArray")?
            .value(row)
            .to_string()),
        DataType::Int32 => Ok(array
            .as_any()
            .downcast_ref::<Int32Array>()
            .context("Int32 column should be Int32Array")?
            .value(row)
            .to_string()),
        DataType::Int64 => Ok(array
            .as_any()
            .downcast_ref::<Int64Array>()
            .context("Int64 column should be Int64Array")?
            .value(row)
            .to_string()),
        DataType::UInt32 => Ok(array
            .as_any()
            .downcast_ref::<UInt32Array>()
            .context("UInt32 column should be UInt32Array")?
            .value(row)
            .to_string()),
        DataType::UInt64 => Ok(array
            .as_any()
            .downcast_ref::<UInt64Array>()
            .context("UInt64 column should be UInt64Array")?
            .value(row)
            .to_string()),
        DataType::Float32 => Ok(array
            .as_any()
            .downcast_ref::<Float32Array>()
            .context("Float32 column should be Float32Array")?
            .value(row)
            .to_string()),
        DataType::Float64 => Ok(array
            .as_any()
            .downcast_ref::<Float64Array>()
            .context("Float64 column should be Float64Array")?
            .value(row)
            .to_string()),
        DataType::Utf8 => Ok(array
            .as_any()
            .downcast_ref::<StringArray>()
            .context("Utf8 column should be StringArray")?
            .value(row)
            .to_string()),
        DataType::Binary => Ok(format!(
            "{:?}",
            array
                .as_any()
                .downcast_ref::<BinaryArray>()
                .context("Binary column should be BinaryArray")?
                .value(row)
        )),
        other => bail!("unsupported result type for datasource output: {other}"),
    }
}

/// 将 Instant 起点转换为经过的毫秒数。
fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
