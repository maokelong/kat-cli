use arrow_array::{ArrayRef, Int64Array, StringArray};
use trace_model::{
    assemble_trace_table_batch, empty_trace_table_batch, trace_table_schema, TraceBoundsBuilder,
    TraceBoundsRow, TraceColumnArray,
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

    let batch = builder.finish().expect("trace_bounds batch builds");

    assert_eq!(batch.num_rows(), 1);
    assert_eq!(batch.schema(), trace_table_schema("trace_bounds").unwrap());
}

#[test]
fn builds_empty_batch_from_contract_without_business_rows() {
    let batch = empty_trace_table_batch("trace_bounds").expect("empty batch builds");

    assert_eq!(batch.num_rows(), 0);
    assert_eq!(batch.num_columns(), 4);
    assert_eq!(batch.schema(), trace_table_schema("trace_bounds").unwrap());
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
