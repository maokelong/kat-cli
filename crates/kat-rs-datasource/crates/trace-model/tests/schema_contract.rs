use trace_model::{schema_for_table, table_names, TRACE_TABLE_NAMES};

#[test]
fn exposes_only_verified_protobuf_htrace_tables() {
    assert_eq!(TRACE_TABLE_NAMES.len(), 19);
    assert_eq!(table_names(), TRACE_TABLE_NAMES);

    for table_name in TRACE_TABLE_NAMES {
        assert!(
            schema_for_table(table_name).is_some(),
            "missing schema for {table_name}"
        );
    }
}

#[test]
fn rejects_unmapped_tables() {
    for table_name in [
        "symbols",
        "cpu_usage",
        "diskio",
        "sys_mem_measure",
        "js_heap_nodes",
        "bytrace",
        "ftrace_text",
    ] {
        assert!(
            schema_for_table(table_name).is_none(),
            "unexpected schema for unmapped table {table_name}"
        );
    }
}
