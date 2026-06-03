use kat_rs_datasource::infer_required_tables;

#[test]
fn infers_required_tables_from_simple_sql() {
    assert_eq!(
        infer_required_tables("SELECT COUNT(*) AS slices FROM sched_slice WHERE cpu = 0"),
        vec!["sched_slice".to_string()]
    );
}

#[test]
fn infers_raw_event_without_matching_raw_substring() {
    assert_eq!(
        infer_required_tables("SELECT * FROM raw_event LIMIT 1"),
        vec!["raw_event".to_string()]
    );
}

#[test]
fn ignores_tables_removed_from_exported_trace_model() {
    assert!(infer_required_tables("SELECT * FROM diskio").is_empty());
    assert!(infer_required_tables("SELECT * FROM js_heap_nodes").is_empty());
}
