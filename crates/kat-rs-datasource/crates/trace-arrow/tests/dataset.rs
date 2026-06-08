use std::sync::Arc;

use arrow_array::{Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use trace_arrow::{ArrowTable, TraceDataset};

#[test]
fn dataset_accepts_empty_table_collection() {
    let dataset = TraceDataset::from_tables(Vec::<ArrowTable>::new()).unwrap();

    assert_eq!(dataset.tables().count(), 0);
}

#[test]
fn arrow_table_rejects_batch_with_different_schema() {
    let table_schema = Arc::new(Schema::new(vec![Field::new(
        "timestamp_ns",
        DataType::UInt64,
        false,
    )]));
    let batch_schema = Arc::new(Schema::new(vec![Field::new(
        "timestamp_ns",
        DataType::Int64,
        false,
    )]));
    let batch = RecordBatch::try_new(batch_schema, vec![Arc::new(Int64Array::from(vec![1]))])
        .expect("batch is valid");

    let result = ArrowTable::new("process_event", table_schema, vec![batch]);

    assert!(result.is_err());
}
