use super::{assemble_trace_table_batch, ModelResult, TraceColumnArray};
use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TraceBoundsRow {
    pub trace_id: String,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub clock_domain: String,
}

#[derive(Debug, Default)]
pub struct TraceBoundsBuilder {
    rows: Vec<TraceBoundsRow>,
}

impl TraceBoundsBuilder {
    /// Appends one typed trace bounds row to the pending batch.
    pub fn push(&mut self, row: TraceBoundsRow) {
        self.rows.push(row);
    }

    /// Converts all pending trace bounds rows into a contract-validated RecordBatch.
    pub fn finish(self) -> ModelResult<RecordBatch> {
        let rows = self.rows;
        assemble_trace_table_batch(
            "trace_bounds",
            vec![
                TraceColumnArray::new(
                    "trace_id",
                    Arc::new(StringArray::from_iter_values(
                        rows.iter().map(|row| row.trace_id.as_str()),
                    )) as ArrayRef,
                ),
                TraceColumnArray::new(
                    "start_ts",
                    Arc::new(Int64Array::from_iter(rows.iter().map(|row| row.start_ts)))
                        as ArrayRef,
                ),
                TraceColumnArray::new(
                    "end_ts",
                    Arc::new(Int64Array::from_iter(rows.iter().map(|row| row.end_ts))) as ArrayRef,
                ),
                TraceColumnArray::new(
                    "clock_domain",
                    Arc::new(StringArray::from_iter_values(
                        rows.iter().map(|row| row.clock_domain.as_str()),
                    )) as ArrayRef,
                ),
            ],
        )
    }
}
