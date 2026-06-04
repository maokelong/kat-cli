use arrow_array::{ArrayRef, Int64Array, StringArray};
use trace_model::{
    assemble_trace_table_batch, trace_table_schema, TraceBoundsBuilder, TraceBoundsRow,
    TraceColumnArray, TraceTables,
};

use std::sync::Arc;

#[test]
fn builds_trace_bounds_batch_from_typed_rows() {
    let mut builder = TraceBoundsBuilder::default();
    builder.push(TraceBoundsRow {
        trace_id: "trace:test".to_string(),
        start_ts: Some(100),
        end_ts: Some(200),
        clock_domain: "boottime".to_string(),
    });

    let batch = builder
        .finish()
        .expect("trace_bounds builder succeeds")
        .expect("trace_bounds batch exists");

    assert_eq!(batch.num_rows(), 1);
    assert_eq!(batch.schema(), trace_table_schema("trace_bounds").unwrap());
}

#[test]
fn skips_trace_bounds_batch_when_builder_has_no_rows() {
    let builder = TraceBoundsBuilder::default();
    let batch = builder.finish().expect("trace_bounds builder succeeds");

    assert!(batch.is_none());
}

#[test]
fn trace_tables_only_keep_non_empty_batches() {
    let batch = assemble_trace_table_batch(
        "trace_bounds",
        vec![
            TraceColumnArray::new(
                "trace_id",
                Arc::new(StringArray::from(vec!["trace:test"])) as ArrayRef,
            ),
            TraceColumnArray::new(
                "start_ts",
                Arc::new(Int64Array::from(vec![Some(100)])) as ArrayRef,
            ),
            TraceColumnArray::new(
                "end_ts",
                Arc::new(Int64Array::from(vec![Some(200)])) as ArrayRef,
            ),
            TraceColumnArray::new(
                "clock_domain",
                Arc::new(StringArray::from(vec!["boottime"])) as ArrayRef,
            ),
        ],
    )
    .expect("batch with rows can be assembled");
    let mut tables = TraceTables::default();

    tables.insert("trace_bounds", batch);

    assert_eq!(tables.batches().len(), 1);
    assert!(tables.get("trace_bounds").is_some());
}

#[test]
fn rejects_zero_row_trace_batches() {
    let err = assemble_trace_table_batch(
        "trace_bounds",
        vec![
            TraceColumnArray::new(
                "trace_id",
                Arc::new(StringArray::from(Vec::<&str>::new())) as ArrayRef,
            ),
            TraceColumnArray::new(
                "start_ts",
                Arc::new(Int64Array::from(Vec::<i64>::new())) as ArrayRef,
            ),
            TraceColumnArray::new(
                "end_ts",
                Arc::new(Int64Array::from(Vec::<i64>::new())) as ArrayRef,
            ),
            TraceColumnArray::new(
                "clock_domain",
                Arc::new(StringArray::from(Vec::<&str>::new())) as ArrayRef,
            ),
        ],
    )
    .expect_err("zero-row batch should not be generated");

    assert!(err.to_string().contains("has no rows"));
}

#[test]
fn assembles_columns_in_contract_order() {
    let batch = assemble_trace_table_batch(
        "trace_bounds",
        vec![
            TraceColumnArray::new(
                "clock_domain",
                Arc::new(StringArray::from(vec!["boottime"])) as ArrayRef,
            ),
            TraceColumnArray::new(
                "end_ts",
                Arc::new(Int64Array::from(vec![Some(200)])) as ArrayRef,
            ),
            TraceColumnArray::new(
                "trace_id",
                Arc::new(StringArray::from(vec!["trace:test"])) as ArrayRef,
            ),
            TraceColumnArray::new(
                "start_ts",
                Arc::new(Int64Array::from(vec![Some(100)])) as ArrayRef,
            ),
        ],
    )
    .expect("batch builds");

    let field_names = batch
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        field_names,
        vec!["trace_id", "start_ts", "end_ts", "clock_domain"]
    );
}

#[test]
fn rejects_missing_extra_and_mismatched_columns() {
    let missing = assemble_trace_table_batch(
        "trace_bounds",
        vec![TraceColumnArray::new(
            "trace_id",
            Arc::new(StringArray::from(vec!["trace:test"])) as ArrayRef,
        )],
    )
    .expect_err("missing columns should fail");
    assert!(missing.to_string().contains("missing column"));

    let extra = assemble_trace_table_batch(
        "trace_bounds",
        vec![
            TraceColumnArray::new(
                "trace_id",
                Arc::new(StringArray::from(vec!["trace:test"])) as ArrayRef,
            ),
            TraceColumnArray::new(
                "start_ts",
                Arc::new(Int64Array::from(vec![Some(100)])) as ArrayRef,
            ),
            TraceColumnArray::new(
                "end_ts",
                Arc::new(Int64Array::from(vec![Some(200)])) as ArrayRef,
            ),
            TraceColumnArray::new(
                "clock_domain",
                Arc::new(StringArray::from(vec!["boottime"])) as ArrayRef,
            ),
            TraceColumnArray::new(
                "extra",
                Arc::new(StringArray::from(vec!["unused"])) as ArrayRef,
            ),
        ],
    )
    .expect_err("extra columns should fail");
    assert!(extra.to_string().contains("extra column"));

    let mismatch = assemble_trace_table_batch(
        "trace_bounds",
        vec![
            TraceColumnArray::new("trace_id", Arc::new(Int64Array::from(vec![1])) as ArrayRef),
            TraceColumnArray::new(
                "start_ts",
                Arc::new(Int64Array::from(vec![Some(100)])) as ArrayRef,
            ),
            TraceColumnArray::new(
                "end_ts",
                Arc::new(Int64Array::from(vec![Some(200)])) as ArrayRef,
            ),
            TraceColumnArray::new(
                "clock_domain",
                Arc::new(StringArray::from(vec!["boottime"])) as ArrayRef,
            ),
        ],
    )
    .expect_err("type mismatch should fail");
    assert!(mismatch.to_string().contains("column type mismatch"));
}
