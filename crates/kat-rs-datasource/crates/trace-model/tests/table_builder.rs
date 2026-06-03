use trace_model::{SchedSliceRow, TraceTableBuilder};

#[test]
fn builds_sched_slice_batch() {
    let mut builder = TraceTableBuilder::default();
    builder.push_sched_slice(SchedSliceRow {
        cpu: 0,
        utid: 1,
        ts: 100,
        dur: Some(50),
        priority: Some(120),
        end_state: Some("S".to_string()),
    });

    let tables = builder
        .finish(
            "test".to_string(),
            Some(100),
            Some(150),
            "boottime".to_string(),
        )
        .unwrap();

    assert_eq!(tables.sched_slice.num_rows(), 1);
    assert_eq!(tables.sched_slice.num_columns(), 6);
    assert_eq!(tables.trace_metadata.num_rows(), 0);
}
